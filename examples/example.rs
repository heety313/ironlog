use ironlog;

fn main() {
    ironlog::TcpLogger::init("127.0.0.1:5000", "4cpp you know me", log::LevelFilter::Debug).unwrap();

    for i in 0..1000 {
        log::info!("Application started - Iteration {}", i);
    }
}