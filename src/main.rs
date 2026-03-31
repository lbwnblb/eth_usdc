use log::{error, info, warn};
use crate::utils::get_env;

mod ed25519;
mod utils;
mod logger;

#[tokio::main]
async fn main() {
    logger::init_logger("logs").expect("日志系统初始化失败");

    info!("服务启动成功 [env={}]", get_env());
    warn!("磁盘空间不足 20%");
    error!("数据库连接超时");
    info!("处理请求 #1024");
}