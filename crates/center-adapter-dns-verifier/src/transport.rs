use std::{
    fmt,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    time::{Duration, Instant},
};

use async_trait::async_trait;
use hickory_proto::{
    op::{Edns, Message, MessageType, OpCode, Query},
    rr::{Name, RecordType},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpStream, UdpSocket},
    time::timeout,
};

const DNS_PORT: u16 = 53;
const MAX_DNS_MESSAGE_SIZE: usize = u16::MAX as usize;
const EDNS_UDP_PAYLOAD_SIZE: u16 = 1232;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DnsTransportProtocol {
    Udp,
    Tcp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DnsQuestion {
    pub name: String,
    pub record_type: RecordType,
    pub dnssec_ok: bool,
    pub recursion_desired: bool,
}

impl DnsQuestion {
    pub fn authoritative(name: impl Into<String>, record_type: RecordType) -> Self {
        Self {
            name: name.into(),
            record_type,
            dnssec_ok: true,
            recursion_desired: false,
        }
    }

    pub fn recursive(name: impl Into<String>, record_type: RecordType) -> Self {
        Self {
            name: name.into(),
            record_type,
            dnssec_ok: true,
            recursion_desired: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IpNetwork {
    pub address: IpAddr,
    pub prefix_len: u8,
}

impl IpNetwork {
    pub fn new(address: IpAddr, prefix_len: u8) -> Result<Self, DnsTransportError> {
        let max = if address.is_ipv4() { 32 } else { 128 };
        if prefix_len > max {
            return Err(DnsTransportError::InvalidPolicy);
        }
        Ok(Self {
            address,
            prefix_len,
        })
    }

    pub fn contains(&self, candidate: IpAddr) -> bool {
        match (self.address, candidate) {
            (IpAddr::V4(network), IpAddr::V4(candidate)) => {
                let mask = if self.prefix_len == 0 {
                    0
                } else {
                    u32::MAX << (32 - self.prefix_len)
                };
                u32::from(network) & mask == u32::from(candidate) & mask
            }
            (IpAddr::V6(network), IpAddr::V6(candidate)) => {
                let mask = if self.prefix_len == 0 {
                    0
                } else {
                    u128::MAX << (128 - self.prefix_len)
                };
                u128::from(network) & mask == u128::from(candidate) & mask
            }
            _ => false,
        }
    }
}

/// The target policy is evaluated immediately before every socket connect.
/// Public verification is fixed to port 53. Private and split-horizon access
/// must opt into both the network and port; there is no public fallback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DnsTargetPolicy {
    PublicDns,
    Explicit {
        allowed_networks: Vec<IpNetwork>,
        allowed_ports: Vec<u16>,
    },
}

impl DnsTargetPolicy {
    pub fn permits(&self, endpoint: SocketAddr) -> bool {
        match self {
            Self::PublicDns => endpoint.port() == DNS_PORT && is_public_unicast(endpoint.ip()),
            Self::Explicit {
                allowed_networks,
                allowed_ports,
            } => {
                is_connectable_unicast(endpoint.ip())
                    && !allowed_networks.is_empty()
                    && allowed_ports.contains(&endpoint.port())
                    && allowed_networks
                        .iter()
                        .any(|network| network.contains(endpoint.ip()))
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DnsTransportError {
    TargetDenied,
    InvalidPolicy,
    InvalidQuestion,
    Timeout,
    Io,
    Encode,
    Decode,
    ResponseMismatch,
    ResponseTooLarge,
}

impl fmt::Display for DnsTransportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::TargetDenied => "DNS target is denied by policy",
            Self::InvalidPolicy => "DNS target policy is invalid",
            Self::InvalidQuestion => "DNS question is invalid",
            Self::Timeout => "DNS exchange timed out",
            Self::Io => "DNS transport failed",
            Self::Encode => "DNS request encoding failed",
            Self::Decode => "DNS response decoding failed",
            Self::ResponseMismatch => "DNS response does not match the request",
            Self::ResponseTooLarge => "DNS response exceeds the bounded message size",
        })
    }
}

impl std::error::Error for DnsTransportError {}

#[derive(Debug, Clone)]
pub struct DnsWireResponse {
    pub endpoint: SocketAddr,
    pub protocol: DnsTransportProtocol,
    pub message: Message,
}

#[async_trait]
pub trait DnsQueryTransport: Send + Sync {
    async fn query(
        &self,
        endpoint: SocketAddr,
        target_policy: &DnsTargetPolicy,
        question: &DnsQuestion,
        exchange_timeout: Duration,
    ) -> Result<DnsWireResponse, DnsTransportError>;
}

#[derive(Debug, Default)]
pub struct TokioDnsQueryTransport;

#[async_trait]
impl DnsQueryTransport for TokioDnsQueryTransport {
    async fn query(
        &self,
        endpoint: SocketAddr,
        target_policy: &DnsTargetPolicy,
        question: &DnsQuestion,
        exchange_timeout: Duration,
    ) -> Result<DnsWireResponse, DnsTransportError> {
        // Deliberately repeat this check in each exchange. A caller must pass
        // an already resolved address, so DNS rebinding cannot change the
        // socket target between validation and connect.
        if !target_policy.permits(endpoint) {
            return Err(DnsTransportError::TargetDenied);
        }
        let (request, expected_query) = build_request(question)?;
        let encoded = request.to_vec().map_err(|_| DnsTransportError::Encode)?;
        let id = request.metadata.id;

        let deadline = Instant::now() + exchange_timeout;
        let udp_bytes = timeout(exchange_timeout, exchange_udp(endpoint, &encoded))
            .await
            .map_err(|_| DnsTransportError::Timeout)??;
        let udp = decode_and_validate(&udp_bytes, id, &expected_query)?;
        if !udp.metadata.truncation {
            return Ok(DnsWireResponse {
                endpoint,
                protocol: DnsTransportProtocol::Udp,
                message: udp,
            });
        }

        if !target_policy.permits(endpoint) {
            return Err(DnsTransportError::TargetDenied);
        }
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .filter(|remaining| !remaining.is_zero())
            .ok_or(DnsTransportError::Timeout)?;
        let tcp_bytes = timeout(remaining, exchange_tcp(endpoint, &encoded))
            .await
            .map_err(|_| DnsTransportError::Timeout)??;
        let tcp = decode_and_validate(&tcp_bytes, id, &expected_query)?;
        if tcp.metadata.truncation {
            return Err(DnsTransportError::Decode);
        }
        Ok(DnsWireResponse {
            endpoint,
            protocol: DnsTransportProtocol::Tcp,
            message: tcp,
        })
    }
}

fn build_request(question: &DnsQuestion) -> Result<(Message, Query), DnsTransportError> {
    let name = Name::from_ascii(&question.name).map_err(|_| DnsTransportError::InvalidQuestion)?;
    if !name.is_fqdn() {
        return Err(DnsTransportError::InvalidQuestion);
    }
    let query = Query::query(name, question.record_type);
    let mut message = Message::new(rand::random(), MessageType::Query, OpCode::Query);
    message.metadata.recursion_desired = question.recursion_desired;
    message.add_query(query.clone());
    let mut edns = Edns::new();
    edns.set_max_payload(EDNS_UDP_PAYLOAD_SIZE)
        .set_dnssec_ok(question.dnssec_ok);
    message.set_edns(edns);
    Ok((message, query))
}

fn decode_and_validate(
    bytes: &[u8],
    expected_id: u16,
    expected_query: &Query,
) -> Result<Message, DnsTransportError> {
    let response = Message::from_vec(bytes).map_err(|_| DnsTransportError::Decode)?;
    if response.metadata.id != expected_id
        || response.metadata.message_type != MessageType::Response
        || response.metadata.op_code != OpCode::Query
        || response.queries.len() != 1
        || response.queries.first() != Some(expected_query)
    {
        return Err(DnsTransportError::ResponseMismatch);
    }
    Ok(response)
}

async fn exchange_udp(endpoint: SocketAddr, encoded: &[u8]) -> Result<Vec<u8>, DnsTransportError> {
    let bind = if endpoint.is_ipv4() {
        SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0))
    } else {
        SocketAddr::from((Ipv6Addr::UNSPECIFIED, 0))
    };
    let socket = UdpSocket::bind(bind)
        .await
        .map_err(|_| DnsTransportError::Io)?;
    socket
        .connect(endpoint)
        .await
        .map_err(|_| DnsTransportError::Io)?;
    socket
        .send(encoded)
        .await
        .map_err(|_| DnsTransportError::Io)?;
    let mut response = vec![0; MAX_DNS_MESSAGE_SIZE];
    let length = socket
        .recv(&mut response)
        .await
        .map_err(|_| DnsTransportError::Io)?;
    response.truncate(length);
    Ok(response)
}

async fn exchange_tcp(endpoint: SocketAddr, encoded: &[u8]) -> Result<Vec<u8>, DnsTransportError> {
    let mut stream = TcpStream::connect(endpoint)
        .await
        .map_err(|_| DnsTransportError::Io)?;
    let length = u16::try_from(encoded.len()).map_err(|_| DnsTransportError::ResponseTooLarge)?;
    stream
        .write_all(&length.to_be_bytes())
        .await
        .map_err(|_| DnsTransportError::Io)?;
    stream
        .write_all(encoded)
        .await
        .map_err(|_| DnsTransportError::Io)?;
    let response_length = stream.read_u16().await.map_err(|_| DnsTransportError::Io)? as usize;
    if response_length == 0 || response_length > MAX_DNS_MESSAGE_SIZE {
        return Err(DnsTransportError::ResponseTooLarge);
    }
    let mut response = vec![0; response_length];
    stream
        .read_exact(&mut response)
        .await
        .map_err(|_| DnsTransportError::Io)?;
    Ok(response)
}

pub fn is_public_unicast(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => is_public_ipv4(address),
        IpAddr::V6(address) => is_public_ipv6(address),
    }
}

fn is_public_ipv4(address: Ipv4Addr) -> bool {
    let octets = address.octets();
    !(address.is_unspecified()
        || address.is_loopback()
        || address.is_private()
        || address.is_link_local()
        || address.is_multicast()
        || address.is_broadcast()
        || octets[0] == 0
        || octets[0] >= 240
        || (octets[0] == 100 && (64..=127).contains(&octets[1]))
        || (octets[0] == 192 && octets[1] == 0 && octets[2] == 0)
        || (octets[0] == 192 && octets[1] == 0 && octets[2] == 2)
        || (octets[0] == 192 && octets[1] == 88 && octets[2] == 99)
        || (octets[0] == 198 && (octets[1] == 18 || octets[1] == 19))
        || (octets[0] == 198 && octets[1] == 51 && octets[2] == 100)
        || (octets[0] == 203 && octets[1] == 0 && octets[2] == 113))
}

fn is_public_ipv6(address: Ipv6Addr) -> bool {
    let segments = address.segments();
    is_connectable_unicast(IpAddr::V6(address))
        && (segments[0] & 0xe000) == 0x2000
        && !((segments[0] & 0xfe00) == 0xfc00
            || (segments[0] & 0xffc0) == 0xfe80
            || (segments[0] == 0x2001 && segments[1] <= 0x01ff)
            || (segments[0] == 0x2001 && segments[1] == 0x0db8)
            || segments[0] == 0x2002
            || (segments[0] == 0x3fff && segments[1] <= 0x0fff)
            || segments[0] == 0x5f00
            || address
                .to_ipv4_mapped()
                .is_some_and(|mapped| !is_public_ipv4(mapped)))
}

fn is_connectable_unicast(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => {
            !address.is_unspecified()
                && !address.is_broadcast()
                && !address.is_multicast()
                && address.octets()[0] != 0
        }
        IpAddr::V6(address) => !address.is_unspecified() && !address.is_multicast(),
    }
}
