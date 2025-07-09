use torpc::tor::TorService;

#[test]
fn test_tor_configuration_check() {
    let tor = TorService::new();
    
    // This should succeed if configs/torrc exists
    let result = tor.check_configuration();
    
    // We expect this to succeed since we created the config
    assert!(result.is_ok(), "Tor configuration check failed: {:?}", result);
}

#[test]
fn test_tor_hostname_reading() {
    let tor = TorService::new();
    
    // Get hostname (will be None if Tor isn't running)
    let hostname = tor.get_hostname().unwrap();
    
    if tor.is_running() {
        assert!(hostname.is_some());
        let hostname = hostname.unwrap();
        assert!(hostname.ends_with(".onion"));
        assert!(hostname.len() > 16); // v3 addresses are longer
    } else {
        assert!(hostname.is_none());
    }
}