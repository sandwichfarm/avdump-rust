//! Drives one stream through the producer and all selected block consumers, and the collection
//! that does so for every provided stream in parallel.

use super::block_stream::{BlockStream, BlockStreamReader};
use super::consumers::{process_blocks, BlockConsumer, BlockConsumerFactory, BlockConsumerSetup};
use super::mirrored_buffer::MirroredBufferPool;
use super::progress::BytesReadProgress;
use super::stream_provider::{ProvidedStream, StreamProvider};
use super::{CancelToken, ProcessingError, ProcessingErrorKind};
use std::io::{Seek, SeekFrom};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

static NEXT_STREAM_ID: AtomicU64 = AtomicU64::new(1);

/// The result of running one consumer over one stream.
pub struct ConsumerOutcome {
    pub name: String,
    pub consumer: Box<dyn BlockConsumer>,
    pub error: Option<ProcessingError>,
}

/// Everything the post-processing stage needs to know about a finished stream.
pub struct StreamResult {
    pub tag: String,
    pub length: u64,
    pub outcomes: Vec<ConsumerOutcome>,
    /// Errors that persisted after all retries (empty on success).
    pub errors: Vec<ProcessingError>,
    pub ran_to_completion: bool,
}

/// Selects consumers by name and constructs the per-stream pipeline.
pub struct StreamConsumerFactory {
    factories: Vec<BlockConsumerFactory>,
    /// Consumer names selected for processing, with their arguments.
    selected: Vec<(String, Vec<String>)>,
    buffer_pool: Arc<MirroredBufferPool>,
    pub min_producer_read_length: usize,
    pub max_producer_read_length: usize,
}

impl StreamConsumerFactory {
    pub fn new(factories: Vec<BlockConsumerFactory>, selected: Vec<(String, Vec<String>)>, buffer_pool: Arc<MirroredBufferPool>, min_producer_read_length: usize, max_producer_read_length: usize) -> Self {
        Self { factories, selected, buffer_pool, min_producer_read_length, max_producer_read_length }
    }

    /// The factories that are selected, in registry order.
    pub fn selected_factories(&self) -> Vec<(&BlockConsumerFactory, &[String])> {
        self.factories
            .iter()
            .filter_map(|f| self.selected.iter().find(|(n, _)| n.eq_ignore_ascii_case(&f.name)).map(|(_, args)| (f, args.as_slice())))
            .collect()
    }

    pub fn selected_names(&self) -> Vec<String> {
        self.selected_factories().iter().map(|(f, _)| f.name.clone()).collect()
    }

    /// Process `provided` on the current thread. `progress` is informed about start/finish.
    pub fn consume(&self, provided: &mut ProvidedStream, progress: &BytesReadProgress, ct: &CancelToken) -> Result<Option<(Vec<ConsumerOutcome>, bool)>, ProcessingError> {
        let selected = self.selected_factories();
        if selected.is_empty() {
            return Ok(None);
        }
        let id = NEXT_STREAM_ID.fetch_add(1, Ordering::Relaxed);
        let buffer = self.buffer_pool.take()?;
        let stream = Arc::new(BlockStream::new(Arc::clone(&buffer), selected.len(), provided.length, self.min_producer_read_length, self.max_producer_read_length, ct.clone()));

        // Create consumers first so that a bad configuration fails before any thread starts.
        let mut consumers: Vec<(Box<dyn BlockConsumer>, BlockStreamReader)> = Vec::with_capacity(selected.len());
        for (index, (factory, args)) in selected.iter().enumerate() {
            let reader = BlockStreamReader::new(Arc::clone(&stream), index);
            let setup = BlockConsumerSetup { name: &factory.name, reader: &reader, tag: &provided.tag, arguments: args };
            match factory.create(&setup) {
                Ok(c) => consumers.push((c, reader)),
                Err(e) => {
                    self.buffer_pool.release(buffer);
                    return Err(ProcessingError::consumer(format!("Could not create consumer {}", factory.name)).with_cause(e));
                }
            }
        }

        let names: Vec<String> = selected.iter().map(|(f, _)| f.name.clone()).collect();
        progress.register(id, &provided.tag, provided.length, Arc::clone(&stream), names);

        let producer_result = Mutex::new(Ok(()));
        let mut outcomes: Vec<ConsumerOutcome> = Vec::with_capacity(consumers.len());
        std::thread::scope(|s| {
            let stream_ref = &stream;
            let source = &mut provided.stream;
            let producer_result = &producer_result;
            s.spawn(move || {
                let r = stream_ref.produce(source.as_mut());
                *producer_result.lock().unwrap_or_else(|e| e.into_inner()) = r;
            });
            let handles: Vec<_> = consumers
                .into_iter()
                .map(|(mut consumer, mut reader)| {
                    let ct = ct.clone();
                    s.spawn(move || {
                        let error = process_blocks(consumer.as_mut(), &mut reader, &ct);
                        ConsumerOutcome { name: consumer.name().to_string(), consumer, error }
                    })
                })
                .collect();
            for h in handles {
                match h.join() {
                    Ok(o) => outcomes.push(o),
                    Err(_) => outcomes.push(ConsumerOutcome { name: "?".to_string(), consumer: Box::new(PanickedConsumer), error: Some(ProcessingError::consumer("Consumer thread panicked")) }),
                }
            }
        });

        drop(stream);
        self.buffer_pool.release(buffer);

        let producer_result = producer_result.into_inner().unwrap_or_else(|e| e.into_inner());
        let cancelled = ct.is_cancelled() || outcomes.iter().any(|o| o.error.as_ref().map(|e| e.is_cancelled()).unwrap_or(false)) || producer_result.as_ref().err().map(|e| e.is_cancelled()).unwrap_or(false);
        if cancelled {
            progress.finished(id, false);
            return Err(ProcessingError::cancelled());
        }
        let mut ran_to_completion = outcomes.iter().all(|o| o.error.is_none());
        if let Err(e) = producer_result {
            ran_to_completion = false;
            outcomes.push(ConsumerOutcome { name: "<producer>".to_string(), consumer: Box::new(PanickedConsumer), error: Some(e) });
        }
        progress.finished(id, ran_to_completion);
        Ok(Some((outcomes, ran_to_completion)))
    }
}

/// Placeholder consumer used when a worker thread died.
struct PanickedConsumer;
impl BlockConsumer for PanickedConsumer {
    fn name(&self) -> &str {
        "?"
    }
    fn do_work(&mut self, _reader: &mut BlockStreamReader, _ct: &CancelToken) -> Result<(), ProcessingError> {
        Ok(())
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Callback invoked (on the stream's worker thread) after each stream has been processed.
pub type StreamFinishedHandler = dyn Fn(StreamResult) + Send + Sync;

/// Called when a stream fails; return true to retry.
pub type StreamErrorHandler = dyn Fn(&ProcessingError, usize) -> bool + Send + Sync;

/// Pulls streams from a provider and processes each on its own thread.
pub struct StreamConsumerCollection {
    factory: Arc<StreamConsumerFactory>,
}

impl StreamConsumerCollection {
    pub fn new(factory: Arc<StreamConsumerFactory>) -> Self {
        Self { factory }
    }

    pub fn factory(&self) -> &Arc<StreamConsumerFactory> {
        &self.factory
    }

    /// Process every stream. Returns the first fatal error, if any.
    pub fn consume_streams(
        &self,
        provider: &mut dyn StreamProvider,
        progress: Arc<BytesReadProgress>,
        ct: &CancelToken,
        on_error: Arc<StreamErrorHandler>,
        on_finished: Arc<StreamFinishedHandler>,
    ) -> Result<(), ProcessingError> {
        let first_error: Arc<Mutex<Option<ProcessingError>>> = Arc::new(Mutex::new(None));
        let mut handles = Vec::new();
        while let Some(mut provided) = provider.next(ct) {
            if ct.is_cancelled() {
                break;
            }
            if first_error.lock().unwrap_or_else(|e| e.into_inner()).is_some() {
                break;
            }
            let factory = Arc::clone(&self.factory);
            let progress = Arc::clone(&progress);
            let ct = ct.clone();
            let on_error = Arc::clone(&on_error);
            let on_finished = Arc::clone(&on_finished);
            let first_error = Arc::clone(&first_error);
            handles.push(std::thread::spawn(move || {
                let result = Self::consume_one(&factory, &mut provided, &progress, &ct, on_error.as_ref(), on_finished.as_ref());
                if let Err(e) = result {
                    if !e.is_cancelled() {
                        let mut fe = first_error.lock().unwrap_or_else(|e| e.into_inner());
                        if fe.is_none() {
                            *fe = Some(e);
                        }
                    }
                    ct.cancel();
                }
                drop(provided);
            }));
            // Reap finished threads to keep the handle list small.
            handles.retain(|h| !h.is_finished());
        }
        for h in handles {
            let _ = h.join();
        }
        let fe = first_error.lock().unwrap_or_else(|e| e.into_inner()).take();
        match fe {
            Some(e) => Err(e),
            None if ct.is_cancelled() => Err(ProcessingError::cancelled()),
            None => Ok(()),
        }
    }

    fn consume_one(
        factory: &StreamConsumerFactory,
        provided: &mut ProvidedStream,
        progress: &BytesReadProgress,
        ct: &CancelToken,
        on_error: &StreamErrorHandler,
        on_finished: &StreamFinishedHandler,
    ) -> Result<(), ProcessingError> {
        let mut retry_count = 0usize;
        loop {
            if provided.stream.seek(SeekFrom::Start(0)).is_err() {
                return Err(ProcessingError::io(&std::io::Error::other("stream is not seekable")));
            }
            match factory.consume(provided, progress, ct) {
                Ok(None) => {
                    progress.skip(provided.length);
                    on_finished(StreamResult { tag: provided.tag.clone(), length: provided.length, outcomes: Vec::new(), errors: Vec::new(), ran_to_completion: true });
                    return Ok(());
                }
                Ok(Some((outcomes, ran_to_completion))) => {
                    if !ran_to_completion {
                        let errors: Vec<ProcessingError> = outcomes.iter().filter_map(|o| o.error.clone()).collect();
                        let cause = ProcessingError::new(ProcessingErrorKind::Consumer, "StreamConsumer threw an Exception")
                            .with_data("StreamTag", provided.tag.clone())
                            .with_cause(errors.first().cloned().unwrap_or_else(|| ProcessingError::other("unknown")));
                        let retry = on_error(&cause, retry_count);
                        retry_count += 1;
                        if retry {
                            continue;
                        }
                        on_finished(StreamResult { tag: provided.tag.clone(), length: provided.length, outcomes, errors, ran_to_completion: false });
                        return Ok(());
                    }
                    on_finished(StreamResult { tag: provided.tag.clone(), length: provided.length, outcomes, errors: Vec::new(), ran_to_completion: true });
                    return Ok(());
                }
                Err(e) if e.is_cancelled() => return Err(e),
                Err(e) => {
                    let retry = on_error(&e, retry_count);
                    retry_count += 1;
                    if retry {
                        continue;
                    }
                    return Err(ProcessingError::new(ProcessingErrorKind::Consumer, "Refused to handle StreamConsumerException").with_cause(e));
                }
            }
        }
    }
}
