use ironlog::TcpLogger;

fn main() {
    // Create an instance-specific logger
    let my_logger = TcpLogger::new("127.0.0.1:5000", "instance_hash", false).unwrap();

    // Use instance-specific logger
    my_logger.info("This is an instance-specific log");
    my_logger.error("This is an error message");

    // Use instance-specific logger in a loop
    for i in 0..5 {
        my_logger.info(&format!("Instance-specific log - Iteration {}", i));
    }
}