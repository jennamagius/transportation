fn main() {
    jenna_reactor::Reactor::run(|reactor| {
        let mut accepted_connections_count = 0;

        let mut listener = ::mio::net::TcpListener::bind(&"0.0.0.0:7777".parse().unwrap())
            .expect("failed to bind");
        let tcp_token = reactor.issue_token();

        reactor
            .poll
            .register(
                &listener,
                tcp_token,
                ::mio::Ready::readable(),
                ::mio::PollOpt::level(),
            )
            .expect("failed to register");

        reactor.set_event_listener(tcp_token, move |reactor, _| {
            let connection = listener.accept().expect("failed to accept");
            accepted_connections_count += 1;
            println!("{:?}, {:?}", connection, accepted_connections_count);
            if accepted_connections_count >= 4 {
                reactor.remove_event_listener(tcp_token);
            }
        });

        let canceller = reactor.set_interval(::std::time::Duration::from_millis(1000), |_| {
            println!("banana")
        });
        reactor.set_timeout(::std::time::Duration::from_millis(9000), move |_| {
            canceller.cancel();
        });
    });
}
