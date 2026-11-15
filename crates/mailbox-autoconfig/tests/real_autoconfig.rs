//! Opt-in, read-only network test: `MAILBOX_TEST_AUTOCONFIG_DOMAIN=<domain> cargo test -p
//! mailbox-autoconfig --test real_autoconfig -- --ignored --nocapture`.
//! Looks the domain up (DNS, HTTPS autoconfig, host-name guesses) and checks that every endpoint it
//! returns passes the TLS probe: valid certificate and a protocol greeting. Sends no credentials.
#![cfg(not(target_arch = "wasm32"))]
use mailbox_autoconfig::system::{SystemResolver, TlsProbe, detect_system};
use mailbox_autoconfig::{Probe, Service};

#[tokio::test]
#[ignore = "needs network and MAILBOX_TEST_AUTOCONFIG_DOMAIN"]
async fn detected_hosts_pass_the_tls_probe() {
    let domain = std::env::var("MAILBOX_TEST_AUTOCONFIG_DOMAIN")
        .expect("set MAILBOX_TEST_AUTOCONFIG_DOMAIN to a domain you may probe");
    let d = detect_system(&domain).await;
    // Hosts and sources only; nothing about any account is printed.
    println!("provider={:?} notes={:?}", d.provider, d.notes);
    let probe = TlsProbe {
        resolver: SystemResolver,
    };
    let mut checked = 0;
    for (found, service) in [(&d.imap, Service::Imap), (&d.smtp, Service::Smtp)] {
        if let Some(f) = found {
            println!(
                "{service:?}: {}:{} {} via {}",
                f.endpoint.host,
                f.endpoint.port,
                f.endpoint.security.label(),
                f.source.key()
            );
            assert!(
                probe.probe(&f.endpoint, service).await,
                "{service:?} endpoint failed the TLS probe"
            );
            checked += 1;
        }
    }
    assert!(checked > 0, "nothing detected for {domain}: {:?}", d.notes);
}
