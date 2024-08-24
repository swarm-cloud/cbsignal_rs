use tklog::{
    LEVEL, LOG,
    Format,MODE,
};
use crate::config::Log;
use crate::config::LogLevel;

pub fn init(log_config: Log) {
    let filename = format!("{:}/signalhub.log", log_config.logger_dir);
    let level = match log_config.logger_level {
        LogLevel::DEBUG => { LEVEL::Debug }
        LogLevel::INFO => { LEVEL::Info }
        LogLevel::WARN => { LEVEL::Warn }
        LogLevel::ERROR => { LEVEL::Error }
        LogLevel::FATAL => { LEVEL::Fatal }
    };
    if log_config.writers == "file" {
        LOG.set_console(false);
        if log_config.log_rotate_date > 0 {
            LOG.set_cutmode_by_time(filename.as_str(), MODE::DAY, log_config.log_rotate_date, true);
        }
        if log_config.log_rotate_size > 0 {
            LOG.set_cutmode_by_size(filename.as_str(), log_config.log_rotate_size*1024*1024, log_config.log_rotate_date, true);
        }
    }
    LOG.set_level(level)
        .set_format(Format::LevelFlag | Format::Time)
        .set_formatter("{level} {time} {message}\n");
}