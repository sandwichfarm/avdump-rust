//! Renders buffer fill bars, per-file progress and the total throughput/ETA line.

use super::console::{ProgressFrame, TICK_PERIOD};
use crate::processing::progress::{BytesReadProgress, Progress};
use std::sync::Arc;
use std::time::Instant;

const UPDATE_PERIOD_IN_TICKS: u32 = 5;
const MEAN_AVERAGE_MINUTE_INTERVAL: f32 = 60.0 * 1000.0 / 100.0;

#[derive(Clone, Copy, Default)]
pub struct DisplayOptions {
    pub hide_buffers: bool,
    pub hide_file_progress: bool,
    pub hide_total_progress: bool,
}

pub struct ProgressDisplay {
    progress: Arc<BytesReadProgress>,
    options: DisplayOptions,
    pub total_bytes: u64,
    pub total_files: usize,
    max_bc_count: usize,
    max_f_count: usize,
    state: u32,
    cur: Progress,
    prev: Progress,
    total_speed_averages: [f32; 3],
    total_speed_display_averages: [i64; 3],
    total_speed_display_unit: &'static str,
    total_bytes_shift_count: u32,
    total_bytes_display_unit: &'static str,
}

impl ProgressDisplay {
    pub fn new(progress: Arc<BytesReadProgress>, options: DisplayOptions, total_bytes: u64, total_files: usize) -> Self {
        let mut shift = 0u32;
        let mut tb = total_bytes;
        while tb > 9999 {
            tb >>= 10;
            shift += 10;
        }
        let unit = match shift {
            40 => "TiB",
            30 => "GiB",
            20 => "MiB",
            10 => "KiB",
            _ => "Byt",
        };
        let cur = progress.get_progress();
        Self {
            progress,
            options,
            total_bytes,
            total_files,
            max_bc_count: 0,
            max_f_count: 0,
            state: 0,
            prev: cur.clone(),
            cur,
            total_speed_averages: [0.0; 3],
            total_speed_display_averages: [0; 3],
            total_speed_display_unit: "MiB/s",
            total_bytes_shift_count: shift,
            total_bytes_display_unit: unit,
        }
    }

    pub fn write_progress(&mut self, frame: &mut ProgressFrame) {
        if frame.finished {
            // Final static frame: show the completed state without interpolation.
            self.cur = self.progress.get_progress();
            self.prev = self.cur.clone();
            self.state = 0;
        }
        if self.state == 0 {
            self.prev = std::mem::replace(&mut self.cur, self.progress.get_progress());
        }
        let interpolation = if frame.finished { 1.0 } else { (self.state + 1) as f64 / UPDATE_PERIOD_IN_TICKS as f64 };
        self.state = (self.state + 1) % UPDATE_PERIOD_IN_TICKS;

        let processed_mib_in_interval = ((self.cur.bytes_processed.saturating_sub(self.prev.bytes_processed)) >> 20) as f32 / UPDATE_PERIOD_IN_TICKS as f32;
        let tick_ms = TICK_PERIOD.as_millis() as f32;
        let now = Instant::now();
        let elapsed_min = now.duration_since(self.cur.started_on).as_secs_f64() / 60.0;

        if elapsed_min < 1.0 {
            let prev_speed = (self.prev.bytes_processed >> 20) as f64 / now.duration_since(self.prev.started_on).as_secs_f64().max(1e-3);
            let cur_speed = (self.cur.bytes_processed >> 20) as f64 / now.duration_since(self.cur.started_on).as_secs_f64().max(1e-3);
            self.total_speed_averages[0] = (prev_speed + interpolation * (cur_speed - prev_speed)) as f32;
        } else {
            let ma = MEAN_AVERAGE_MINUTE_INTERVAL;
            self.total_speed_averages[0] = self.total_speed_averages[0] * (ma - 1.0) / ma + (processed_mib_in_interval / tick_ms * 1000.0) / ma;
        }
        if elapsed_min < 5.0 {
            self.total_speed_averages[1] = self.total_speed_averages[0];
        } else {
            let ma = MEAN_AVERAGE_MINUTE_INTERVAL * 5.0;
            self.total_speed_averages[1] = self.total_speed_averages[1] * (ma - 1.0) / ma + (processed_mib_in_interval / tick_ms * 1000.0) / ma;
        }
        if elapsed_min < 15.0 {
            self.total_speed_averages[2] = self.total_speed_averages[1];
        } else {
            let ma = MEAN_AVERAGE_MINUTE_INTERVAL * 15.0;
            self.total_speed_averages[2] = self.total_speed_averages[2] * (ma - 1.0) / ma + (processed_mib_in_interval / tick_ms * 1000.0) / ma;
        }
        let a = self.total_speed_averages;
        if a.iter().any(|&x| x > 9_999_999.0) {
            self.total_speed_display_averages = [(a[0] as i64) >> 20, (a[1] as i64) >> 20, (a[2] as i64) >> 20];
            self.total_speed_display_unit = "TiB/s";
        } else if a.iter().any(|&x| x > 9999.0) {
            self.total_speed_display_averages = [(a[0] as i64) >> 10, (a[1] as i64) >> 10, (a[2] as i64) >> 10];
            self.total_speed_display_unit = "GiB/s";
        } else {
            self.total_speed_display_averages = [a[0] as i64, a[1] as i64, a[2] as i64];
            self.total_speed_display_unit = "MiB/s";
        }

        self.display(frame, interpolation);
        frame.special_jitter_event = self.state == 0;
    }

    fn display(&mut self, sb: &mut ProgressFrame, rel_pos: f64) {
        let console_width = sb.display_width;
        if console_width < 72 {
            return;
        }
        let now = Instant::now();
        sb.append_char('-', console_width - 2).append_line();

        if !self.options.hide_buffers && !sb.finished {
            let bar_width = console_width - 8 - 1 - 2 - 2;
            let mut cur_bc_count = 0;
            for (i, cur) in self.cur.block_consumer_progress.iter().enumerate() {
                let prev_fill = self.prev.block_consumer_progress.get(i).map(|p| p.buffer_fill).unwrap_or(cur.buffer_fill);
                let prev_active = self.prev.block_consumer_progress.get(i).map(|p| p.active_count).unwrap_or(0);
                if cur.active_count == 0 && prev_active == 0 {
                    continue;
                }
                cur_bc_count += 1;
                sb.append_fixed(&cur.name, 8).append(" ");
                sb.append_bar(bar_width, prev_fill + rel_pos * (cur.buffer_fill - prev_fill)).append(" ");
                sb.append(&cur.active_count.to_string()).append_line();
            }
            if self.max_bc_count < cur_bc_count {
                self.max_bc_count = cur_bc_count;
            }
            for _ in cur_bc_count..self.max_bc_count + 1 {
                sb.append_line();
            }
        }

        if !self.options.hide_file_progress && !sb.finished {
            let bar_width = console_width - 23;
            let mut items: Vec<(&crate::processing::progress::FileProgress, &crate::processing::progress::FileProgress)> = self
                .cur
                .file_progress
                .iter()
                .map(|cur| (cur, self.prev.file_progress.iter().find(|p| p.id == cur.id).unwrap_or(cur)))
                .collect();
            items.sort_by_key(|(c, _)| c.started_on);
            let cur_f_count = items.len();
            let bc_count = self.cur.block_consumer_progress.len().max(1) as u32;
            for (cur, prev) in items {
                let file_name = crate::misc::file_name(std::path::Path::new(&cur.file_path));
                sb.append_fixed(&file_name, bar_width);

                let writer_lock = cur.writer_lock_count.saturating_sub(prev.writer_lock_count);
                let reader_lock = cur.reader_lock_count.saturating_sub(prev.reader_lock_count) / bc_count;
                let lock_sign = if writer_lock == 0 && reader_lock == 0 { ' ' } else if writer_lock < reader_lock { '-' } else { '+' };
                sb.append_char(lock_sign, 1);

                let factor = if prev.file_length > 0 {
                    let fp = prev.bytes_processed as f64 / prev.file_length as f64;
                    let fc = cur.bytes_processed as f64 / cur.file_length.max(1) as f64;
                    fp + rel_pos * (fc - fp)
                } else {
                    1.0
                };
                sb.append_bar(10, factor);

                let prev_speed = (prev.bytes_processed >> 20) as f64 / now.duration_since(prev.started_on).as_secs_f64().max(1e-3);
                let cur_speed = (cur.bytes_processed >> 20) as f64 / now.duration_since(cur.started_on).as_secs_f64().max(1e-3);
                let speed = (prev_speed + rel_pos * (cur_speed - prev_speed)) as i64;
                sb.append(&format!("{:>3}", speed.min(999))).append("MiB/s").append_line();
            }
            if self.max_f_count < cur_f_count {
                self.max_f_count = cur_f_count;
            }
            for _ in cur_f_count..self.max_f_count {
                sb.append_line();
            }
        }

        if !self.options.hide_total_progress && self.total_bytes > 0 {
            let prev_factor = (self.prev.bytes_processed as f64 / self.total_bytes as f64).clamp(0.0, 1.0);
            let mut cur_factor = (self.cur.bytes_processed as f64 / self.total_bytes as f64).clamp(0.0, 1.0);
            if prev_factor > cur_factor {
                cur_factor = prev_factor;
            }
            let factor = prev_factor + rel_pos * (cur_factor - prev_factor);
            sb.append("Total ").append_bar(console_width - 31, factor).append(" ");
            sb.append_pad_left(self.total_speed_display_averages[0].min(9999), 5);
            sb.append_pad_left(self.total_speed_display_averages[1].min(9999), 5);
            sb.append_pad_left(self.total_speed_display_averages[2].min(9999), 5);
            sb.append(self.total_speed_display_unit).append_line();

            let mut eta_str = "-.--:--:--".to_string();
            if self.total_speed_averages[2] != 0.0 {
                let remaining_mib = (self.total_bytes.saturating_sub(self.cur.bytes_processed) >> 20) as f64;
                let eta_secs = remaining_mib / self.total_speed_averages[2] as f64;
                if eta_secs.is_finite() && eta_secs / 86_400.0 <= 9.0 {
                    eta_str = format_dhms(eta_secs);
                }
            }
            sb.append(&format!("{}/{} Files | ", self.cur.files_processed, self.total_files));
            sb.append(&format!("{}/{} {} | ", self.cur.bytes_processed >> self.total_bytes_shift_count, self.total_bytes >> self.total_bytes_shift_count, self.total_bytes_display_unit));
            sb.append(&format!("{} Elapsed | ", format_dhms(now.duration_since(self.cur.started_on).as_secs_f64())));
            sb.append(&eta_str).append(" Remaining").append_line();
        }
    }
}

/// `d.hh:mm:ss`
pub fn format_dhms(seconds: f64) -> String {
    let total = seconds.max(0.0) as u64;
    let days = total / 86_400;
    let rem = total % 86_400;
    format!("{}.{:02}:{:02}:{:02}", days, rem / 3600, (rem % 3600) / 60, rem % 60)
}
