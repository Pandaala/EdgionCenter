use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::Arc,
    time::Duration,
};

use edgion_center_adapter_dns_verifier::{
    is_public_unicast, DnsQueryTransport, DnsQuestion, DnsTargetPolicy, DnsTransportError,
    DnsTransportProtocol, IpNetwork, TokioDnsQueryTransport,
};
use hickory_proto::{
    op::Message,
    rr::{rdata::A, RData, Record, RecordType},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, UdpSocket},
};

fn loopback_policy(port: u16) -> DnsTargetPolicy {
    DnsTargetPolicy::Explicit {
        allowed_networks: vec![IpNetwork::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 32).unwrap()],
        allowed_ports: vec![port],
    }
}

fn response_for(request: &Message, truncated: bool) -> Vec<u8> {
    assert!(!request.metadata.recursion_desired);
    assert_eq!(request.queries.len(), 1);
    let query = request.queries[0].clone();
    let mut response = Message::response(request.metadata.id, request.metadata.op_code);
    response.metadata.authoritative = true;
    response.metadata.truncation = truncated;
    response.add_query(query.clone());
    if !truncated {
        response.add_answer(Record::from_rdata(
            query.name().clone(),
            60,
            RData::A(A(Ipv4Addr::new(192, 0, 2, 10))),
        ));
    }
    response.to_vec().unwrap()
}

#[test]
fn public_policy_rejects_special_use_ranges() {
    for denied in [
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
        IpAddr::V4(Ipv4Addr::new(100, 64, 0, 1)),
        IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)),
        IpAddr::V4(Ipv4Addr::new(198, 18, 0, 1)),
        IpAddr::V6(Ipv6Addr::LOCALHOST),
        IpAddr::V6("fc00::1".parse().unwrap()),
        IpAddr::V6("fe80::1".parse().unwrap()),
        IpAddr::V6("2001:db8::1".parse().unwrap()),
        IpAddr::V6("2001:100::1".parse().unwrap()),
        IpAddr::V6("4000::1".parse().unwrap()),
        IpAddr::V6("::c0a8:1".parse().unwrap()),
    ] {
        assert!(
            !is_public_unicast(denied),
            "unexpected public address {denied}"
        );
    }
    assert!(is_public_unicast(IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1))));
    assert!(is_public_unicast(IpAddr::V6(
        "2606:4700:4700::1111".parse().unwrap()
    )));
}

#[test]
fn explicit_policy_may_allow_ipv6_loopback_but_not_unspecified_or_multicast() {
    let policy = DnsTargetPolicy::Explicit {
        allowed_networks: vec![IpNetwork::new(IpAddr::V6(Ipv6Addr::LOCALHOST), 128).unwrap()],
        allowed_ports: vec![5300],
    };
    assert!(policy.permits(SocketAddr::from((Ipv6Addr::LOCALHOST, 5300))));
    assert!(!policy.permits(SocketAddr::from((Ipv6Addr::UNSPECIFIED, 5300))));
    assert!(!policy.permits(SocketAddr::new("ff02::1".parse().unwrap(), 5300)));
}

#[tokio::test]
async fn target_is_denied_before_network_io() {
    let error = TokioDnsQueryTransport
        .query(
            SocketAddr::from((Ipv4Addr::LOCALHOST, 53)),
            &DnsTargetPolicy::PublicDns,
            &DnsQuestion::authoritative("example.com.", RecordType::A),
            Duration::from_millis(50),
        )
        .await
        .unwrap_err();
    assert_eq!(error, DnsTransportError::TargetDenied);
}

#[tokio::test]
async fn authoritative_udp_exchange_validates_the_response() {
    let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let endpoint = socket.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let mut bytes = vec![0; 4096];
        let (length, peer) = socket.recv_from(&mut bytes).await.unwrap();
        let request = Message::from_vec(&bytes[..length]).unwrap();
        socket
            .send_to(&response_for(&request, false), peer)
            .await
            .unwrap();
    });

    let response = TokioDnsQueryTransport
        .query(
            endpoint,
            &loopback_policy(endpoint.port()),
            &DnsQuestion::authoritative("example.com.", RecordType::A),
            Duration::from_secs(1),
        )
        .await
        .unwrap();
    assert_eq!(response.protocol, DnsTransportProtocol::Udp);
    assert!(response.message.metadata.authoritative);
    assert_eq!(response.message.answers.len(), 1);
    server.await.unwrap();
}

#[tokio::test]
async fn truncated_udp_response_retries_the_same_endpoint_over_tcp() {
    let udp = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let endpoint = udp.local_addr().unwrap();
    let tcp = TcpListener::bind(endpoint).await.unwrap();
    let seen_id = Arc::new(tokio::sync::Mutex::new(None));

    let udp_id = Arc::clone(&seen_id);
    let udp_server = tokio::spawn(async move {
        let mut bytes = vec![0; 4096];
        let (length, peer) = udp.recv_from(&mut bytes).await.unwrap();
        let request = Message::from_vec(&bytes[..length]).unwrap();
        *udp_id.lock().await = Some(request.metadata.id);
        udp.send_to(&response_for(&request, true), peer)
            .await
            .unwrap();
    });
    let tcp_id = Arc::clone(&seen_id);
    let tcp_server = tokio::spawn(async move {
        let (mut stream, _) = tcp.accept().await.unwrap();
        let length = stream.read_u16().await.unwrap() as usize;
        let mut bytes = vec![0; length];
        stream.read_exact(&mut bytes).await.unwrap();
        let request = Message::from_vec(&bytes).unwrap();
        assert_eq!(*tcp_id.lock().await, Some(request.metadata.id));
        let response = response_for(&request, false);
        stream
            .write_all(&(response.len() as u16).to_be_bytes())
            .await
            .unwrap();
        stream.write_all(&response).await.unwrap();
    });

    let response = TokioDnsQueryTransport
        .query(
            endpoint,
            &loopback_policy(endpoint.port()),
            &DnsQuestion::authoritative("example.com.", RecordType::A),
            Duration::from_secs(1),
        )
        .await
        .unwrap();
    assert_eq!(response.protocol, DnsTransportProtocol::Tcp);
    assert_eq!(response.message.answers.len(), 1);
    udp_server.await.unwrap();
    tcp_server.await.unwrap();
}

#[tokio::test]
async fn mismatched_transaction_id_is_rejected() {
    let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let endpoint = socket.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let mut bytes = vec![0; 4096];
        let (length, peer) = socket.recv_from(&mut bytes).await.unwrap();
        let request = Message::from_vec(&bytes[..length]).unwrap();
        let mut response = Message::response(
            request.metadata.id.wrapping_add(1),
            request.metadata.op_code,
        );
        response.add_query(request.queries[0].clone());
        socket
            .send_to(&response.to_vec().unwrap(), peer)
            .await
            .unwrap();
    });

    let error = TokioDnsQueryTransport
        .query(
            endpoint,
            &loopback_policy(endpoint.port()),
            &DnsQuestion::authoritative("example.com.", RecordType::A),
            Duration::from_secs(1),
        )
        .await
        .unwrap_err();
    assert_eq!(error, DnsTransportError::ResponseMismatch);
    server.await.unwrap();
}
