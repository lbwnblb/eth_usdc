use log::LevelFilter;
use log4rs::{
    append::{
        console::ConsoleAppender,
        rolling_file::{
            policy::compound::{
                roll::fixed_window::FixedWindowRoller,
                trigger::time::TimeTrigger,
                CompoundPolicy,
            },
            RollingFileAppender,
        },
    },
    config::{Appender, Config, Root},
    encode::pattern::PatternEncoder,
    filter::threshold::ThresholdFilter,
};
use log4rs::append::rolling_file::policy::compound::trigger::time::{TimeTriggerConfig, TimeTriggerInterval};

// ── 日志格式 ──────────────────────────────────────────────────────────────────
const LOG_PATTERN: &str = "{d(%Y-%m-%d %H:%M:%S%.3f)} [{l:<5}] [{T}] {M}:{L} - {m}{n}";

/// 构建单个滚动文件 Appender
///
/// - `base_path`    : 当前日志文件路径，如 "logs/all.log"
/// - `archive_pattern`: 归档模式，如 "logs/archive/{}-all.log"
/// - `level`        : 该 appender 过滤的最低级别
fn build_rolling_appender(
    base_path: &str,
    archive_pattern: &str,
    level: LevelFilter,
) -> (
    Box<dyn log4rs::append::Append>,
    Option<Box<dyn log4rs::filter::Filter>>,
) {
    // 每天触发一次滚动
    let time_trigger = TimeTrigger::new(
        TimeTriggerConfig {
            interval: TimeTriggerInterval::Day(1),
            modulate: true,
            max_random_delay: 0,
        }
    );

    // 保留最多 15 个归档文件（对应 15 天）
    let roller = FixedWindowRoller::builder()
        .build(archive_pattern, 15)
        .expect("创建 FixedWindowRoller 失败");

    let policy = CompoundPolicy::new(Box::new(time_trigger), Box::new(roller));

    let appender = RollingFileAppender::builder()
        .encoder(Box::new(PatternEncoder::new(LOG_PATTERN)))
        .append(true)
        .build(base_path, Box::new(policy))
        .unwrap_or_else(|e| panic!("构建 RollingFileAppender({base_path}) 失败: {e}"));

    let filter = Box::new(ThresholdFilter::new(level));
    (Box::new(appender), Some(filter))
}

/// 初始化日志系统
///
/// 根据环境变量 `get_env()` 的返回值决定输出方式：
///
/// - `dev`  : 所有日志（TRACE 及以上）输出到控制台，不写文件
/// - 其他   : 日志写入文件，目录结构如下：
///
/// ```
/// logs/
///   all.log              ← 当天全量日志（TRACE+）
///   info.log             ← 当天 INFO 日志
///   warn.log             ← 当天 WARN 日志
///   error.log            ← 当天 ERROR 日志
///   archive/
///     {n}-all.log        ← 归档（最多保留 15 天）
///     {n}-info.log
///     {n}-warn.log
///     {n}-error.log
/// ```
pub fn init_logger(log_dir: &str) -> Result<(), Box<dyn std::error::Error>> {
    let env = crate::utils::get_env();
    let config = if env == "dev" {
        // ── dev 模式：全部输出到控制台 ────────────────────────────────────────
        let console = ConsoleAppender::builder()
            .encoder(Box::new(PatternEncoder::new(LOG_PATTERN)))
            .build();

        Config::builder()
            .appender(Appender::builder().build("console", Box::new(console)))
            .build(
                Root::builder()
                    .appender("console")
                    .build(LevelFilter::Info),
            )?
    } else {
        // ── 非 dev 模式：输出到滚动文件 ───────────────────────────────────────
        std::fs::create_dir_all(format!("{log_dir}/archive"))?;

        // all.log —— 全量（Info+）
        let (all_appender, all_filter) = build_rolling_appender(
            &format!("{log_dir}/all.log"),
            &format!("{log_dir}/archive/{{}}-all.log"),
            LevelFilter::Info,
        );
        // info.log —— 仅 INFO（精确过滤）
        let (info_appender, _) = build_rolling_appender(
            &format!("{log_dir}/info.log"),
            &format!("{log_dir}/archive/{{}}-info.log"),
            LevelFilter::Info,
        );
        // warn.log —— 仅 WARN
        let (warn_appender, _) = build_rolling_appender(
            &format!("{log_dir}/warn.log"),
            &format!("{log_dir}/archive/{{}}-warn.log"),
            LevelFilter::Warn,
        );
        // error.log —— 仅 ERROR
        let (error_appender, _) = build_rolling_appender(
            &format!("{log_dir}/error.log"),
            &format!("{log_dir}/archive/{{}}-error.log"),
            LevelFilter::Error,
        );

        Config::builder()
            .appender(
                Appender::builder()
                    .filter(all_filter.unwrap())
                    .build("all", all_appender),
            )
            .appender(
                Appender::builder()
                    .filter(Box::new(ExactLevelFilter(log::Level::Info)))
                    .build("info", info_appender),
            )
            .appender(
                Appender::builder()
                    .filter(Box::new(ExactLevelFilter(log::Level::Warn)))
                    .build("warn", warn_appender),
            )
            .appender(
                Appender::builder()
                    .filter(Box::new(ExactLevelFilter(log::Level::Error)))
                    .build("error", error_appender),
            )
            .build(
                Root::builder()
                    .appender("all")
                    .appender("info")
                    .appender("warn")
                    .appender("error")
                    .build(LevelFilter::Info),
            )?
    };

    log4rs::init_config(config)?;
    Ok(())
}

// ── 精确级别过滤器 ────────────────────────────────────────────────────────────

/// 只允许指定级别的日志通过，屏蔽其他所有级别。
#[derive(Debug)]
struct ExactLevelFilter(log::Level);

impl log4rs::filter::Filter for ExactLevelFilter {
    fn filter(&self, record: &log::Record) -> log4rs::filter::Response {
        if record.level() == self.0 {
            log4rs::filter::Response::Accept
        } else {
            log4rs::filter::Response::Reject
        }
    }
}