use ironlog;

fn main() {
    ironlog::TcpLogger::init("127.0.0.1:5000", "4cpp you know me", log::LevelFilter::Debug).unwrap();

    for _ in 0..1 {
        log::info!("Application started");
        log::warn!("This is a warning");
        log::error!("An error occurred");
        log::debug!("ddddd");
    }

}