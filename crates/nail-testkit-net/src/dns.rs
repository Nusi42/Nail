//! Fake DNS: a [`DnsZone`] of SRV, MX and A records, served over UDP by
//! [`MockDnsServer`] (so a real resolver such as `hickory-resolver` can be
//! pointed at it) or queried directly in memory.
//!
//! Names are compared case-insensitively and without the trailing dot. A
//! name with no records at all answers `NXDOMAIN`; a name with records of
//! other types answers an empty `NOERROR`; names marked with
//! [`DnsZone::servfail`] answer `SERVFAIL`.

use std::collections::{HashMap, HashSet};
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use hickory_proto::op::{Message, ResponseCode};
use hickory_proto::rr::rdata::{A, MX, SRV};
use hickory_proto::rr::{Name, RData, Record, RecordType};
use tokio::net::UdpSocket;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::MockNetError;

/// An SRV record's data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SrvAnswer {
    /// Lower is preferred.
    pub priority: u16,
    /// Relative weight among equal priorities.
    pub weight: u16,
    /// Port.
    pub port: u16,
    /// Target host (`"."` = service not available).
    pub target: String,
}

/// An MX record's data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MxAnswer {
    /// Lower is preferred.
    pub preference: u16,
    /// Mail exchanger host.
    pub exchange: String,
}

/// A set of DNS records.
#[derive(Debug, Clone, Default)]
pub struct DnsZone {
    srv: HashMap<String, Vec<SrvAnswer>>,
    mx: HashMap<String, Vec<MxAnswer>>,
    a: HashMap<String, Vec<Ipv4Addr>>,
    servfail: HashSet<String>,
}

fn key(name: &str) -> String {
    name.trim_end_matches('.').to_ascii_lowercase()
}

/// The outcome of a lookup in a [`DnsZone`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ZoneAnswer<T> {
    /// Records (possibly none of the requested type: empty `NOERROR`).
    Records(Vec<T>),
    /// The name does not exist.
    NxDomain,
    /// The server failed.
    ServFail,
}

impl DnsZone {
    /// An empty zone.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an SRV record, e.g. `("_imaps._tcp.example.org", 0, 1, 993,
    /// "imap.example.org")`.
    pub fn add_srv(
        &mut self,
        name: &str,
        priority: u16,
        weight: u16,
        port: u16,
        target: &str,
    ) -> &mut Self {
        self.srv.entry(key(name)).or_default().push(SrvAnswer {
            priority,
            weight,
            port,
            target: key(target),
        });
        self
    }

    /// Adds an MX record.
    pub fn add_mx(&mut self, domain: &str, preference: u16, exchange: &str) -> &mut Self {
        self.mx.entry(key(domain)).or_default().push(MxAnswer {
            preference,
            exchange: key(exchange),
        });
        self
    }

    /// Adds an A record.
    pub fn add_a(&mut self, name: &str, ip: Ipv4Addr) -> &mut Self {
        self.a.entry(key(name)).or_default().push(ip);
        self
    }

    /// Makes every query for `name` fail with `SERVFAIL`.
    pub fn servfail(&mut self, name: &str) -> &mut Self {
        self.servfail.insert(key(name));
        self
    }

    fn exists(&self, name: &str) -> bool {
        self.srv.contains_key(name) || self.mx.contains_key(name) || self.a.contains_key(name)
    }

    fn answer<T: Clone>(&self, name: &str, table: &HashMap<String, Vec<T>>) -> ZoneAnswer<T> {
        let name = key(name);
        if self.servfail.contains(&name) {
            ZoneAnswer::ServFail
        } else if let Some(records) = table.get(&name) {
            ZoneAnswer::Records(records.clone())
        } else if self.exists(&name) {
            ZoneAnswer::Records(Vec::new())
        } else {
            ZoneAnswer::NxDomain
        }
    }

    /// SRV records of `name`.
    pub fn srv(&self, name: &str) -> ZoneAnswer<SrvAnswer> {
        self.answer(name, &self.srv)
    }

    /// MX records of `name`.
    pub fn mx(&self, name: &str) -> ZoneAnswer<MxAnswer> {
        self.answer(name, &self.mx)
    }

    /// A records of `name`.
    pub fn a(&self, name: &str) -> ZoneAnswer<Ipv4Addr> {
        self.answer(name, &self.a)
    }

    /// The response code and records of a zone answer (records that cannot be
    /// written are left out).
    fn wire_answer<T>(
        answer: ZoneAnswer<T>,
        record: impl Fn(&T) -> Option<RData>,
    ) -> (ResponseCode, Vec<RData>) {
        match answer {
            ZoneAnswer::Records(r) => {
                (ResponseCode::NoError, r.iter().filter_map(record).collect())
            }
            ZoneAnswer::NxDomain => (ResponseCode::NXDomain, Vec::new()),
            ZoneAnswer::ServFail => (ResponseCode::ServFail, Vec::new()),
        }
    }

    /// Answers a DNS query message (wire format) from this zone.
    pub fn respond(&self, request: &[u8]) -> Option<Vec<u8>> {
        let request = Message::from_vec(request).ok()?;
        let mut response = Message::response(request.metadata.id, request.metadata.op_code);
        response.metadata.recursion_desired = request.metadata.recursion_desired;
        response.metadata.recursion_available = true;
        response.metadata.authoritative = true;
        response.queries.clone_from(&request.queries);
        for query in &request.queries {
            let name = query.name.to_string();
            let fqdn = |host: &str| Name::from_ascii(format!("{}.", key(host))).ok();
            let (code, records): (ResponseCode, Vec<RData>) = match query.query_type {
                RecordType::SRV => Self::wire_answer(self.srv(&name), |s| {
                    let target = if s.target.is_empty() {
                        Some(Name::root())
                    } else {
                        fqdn(&s.target)
                    }?;
                    Some(RData::SRV(SRV::new(s.priority, s.weight, s.port, target)))
                }),
                RecordType::MX => Self::wire_answer(self.mx(&name), |m| {
                    Some(RData::MX(MX::new(m.preference, fqdn(&m.exchange)?)))
                }),
                RecordType::A => Self::wire_answer(self.a(&name), |ip| Some(RData::A(A(*ip)))),
                _ if self.exists(&key(&name)) => (ResponseCode::NoError, Vec::new()),
                _ => (ResponseCode::NXDomain, Vec::new()),
            };
            response.metadata.response_code = code;
            for data in records {
                response.add_answer(Record::from_rdata(query.name.clone(), 300, data));
            }
        }
        response.to_vec().ok()
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

#[derive(Debug, Default)]
struct Inner {
    zone: Mutex<DnsZone>,
    queries: Mutex<Vec<(String, String)>>,
}

/// A running UDP DNS server on `127.0.0.1` serving a [`DnsZone`]. Stops when
/// dropped.
#[derive(Debug)]
pub struct MockDnsServer {
    addr: SocketAddr,
    inner: Arc<Inner>,
    shutdown: watch::Sender<bool>,
    task: JoinHandle<()>,
}

impl MockDnsServer {
    /// Starts serving `zone` on a free loopback UDP port.
    pub async fn start(zone: DnsZone) -> Result<Self, MockNetError> {
        let socket = UdpSocket::bind(("127.0.0.1", 0)).await?;
        let addr = socket.local_addr()?;
        let inner = Arc::new(Inner {
            zone: Mutex::new(zone),
            queries: Mutex::default(),
        });
        let (shutdown, mut stop) = watch::channel(false);
        let task_inner = Arc::clone(&inner);
        let task = tokio::spawn(async move {
            let mut buf = vec![0u8; 4096];
            loop {
                tokio::select! {
                    _ = stop.changed() => break,
                    received = socket.recv_from(&mut buf) => {
                        let Ok((n, peer)) = received else { continue };
                        if let Ok(message) = Message::from_vec(&buf[..n]) {
                            let mut log = lock(&task_inner.queries);
                            for q in &message.queries {
                                log.push((q.query_type.to_string(), key(&q.name.to_string())));
                            }
                        }
                        let reply = lock(&task_inner.zone).respond(&buf[..n]);
                        if let Some(reply) = reply {
                            drop(socket.send_to(&reply, peer).await);
                        }
                    }
                }
            }
        });
        Ok(Self {
            addr,
            inner,
            shutdown,
            task,
        })
    }

    /// The UDP address to use as the resolver's name server.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Changes the served records.
    pub fn update(&self, change: impl FnOnce(&mut DnsZone)) {
        change(&mut lock(&self.inner.zone));
    }

    /// Every (type, name) query received, in order.
    pub fn queries(&self) -> Vec<(String, String)> {
        lock(&self.inner.queries).clone()
    }
}

impl Drop for MockDnsServer {
    fn drop(&mut self) {
        let _ = self.shutdown.send(true);
        self.task.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hickory_proto::op::Query;

    fn zone() -> DnsZone {
        let mut zone = DnsZone::new();
        zone.add_srv("_imaps._tcp.Example.org.", 0, 1, 993, "imap.example.org")
            .add_srv("_pop3._tcp.example.org", 0, 0, 0, ".")
            .add_mx("example.org", 10, "mx.provider.net")
            .add_a("imap.example.org", Ipv4Addr::LOCALHOST)
            .servfail("broken.example");
        zone
    }

    #[test]
    fn zone_lookups() {
        let zone = zone();
        assert_eq!(
            zone.srv("_IMAPS._tcp.example.org"),
            ZoneAnswer::Records(vec![SrvAnswer {
                priority: 0,
                weight: 1,
                port: 993,
                target: "imap.example.org".into()
            }])
        );
        assert_eq!(
            zone.srv("_pop3._tcp.example.org"),
            ZoneAnswer::Records(vec![SrvAnswer {
                priority: 0,
                weight: 0,
                port: 0,
                target: String::new(),
            }])
        );
        assert_eq!(zone.srv("_imap._tcp.example.org"), ZoneAnswer::NxDomain);
        assert_eq!(zone.mx("imap.example.org"), ZoneAnswer::Records(vec![]));
        assert_eq!(zone.mx("broken.example"), ZoneAnswer::ServFail);
    }

    fn ask(zone: &DnsZone, name: &str, rtype: RecordType) -> Message {
        let mut query = Message::query();
        query.add_query(Query::query(Name::from_ascii(name).unwrap(), rtype));
        Message::from_vec(&zone.respond(&query.to_vec().unwrap()).unwrap()).unwrap()
    }

    #[test]
    fn wire_answers() {
        let zone = zone();
        let reply = ask(&zone, "_imaps._tcp.example.org.", RecordType::SRV);
        assert_eq!(reply.metadata.response_code, ResponseCode::NoError);
        let RData::SRV(srv) = &reply.answers[0].data else {
            panic!("not SRV")
        };
        assert_eq!(
            (srv.port, srv.target.to_string().as_str()),
            (993, "imap.example.org.")
        );
        let RData::SRV(none) =
            &ask(&zone, "_pop3._tcp.example.org.", RecordType::SRV).answers[0].data
        else {
            panic!("not SRV")
        };
        assert!(none.target.is_root());
        assert_eq!(
            ask(&zone, "nothing.example.", RecordType::SRV)
                .metadata
                .response_code,
            ResponseCode::NXDomain
        );
        assert_eq!(
            ask(&zone, "broken.example.", RecordType::MX)
                .metadata
                .response_code,
            ResponseCode::ServFail
        );
        let RData::MX(mx) = &ask(&zone, "example.org.", RecordType::MX).answers[0].data else {
            panic!("not MX")
        };
        assert_eq!(mx.exchange.to_string(), "mx.provider.net.");
    }

    #[tokio::test]
    async fn serves_udp() {
        let server = MockDnsServer::start(zone()).await.unwrap();
        let socket = UdpSocket::bind(("127.0.0.1", 0)).await.unwrap();
        let mut query = Message::query();
        query.add_query(Query::query(
            Name::from_ascii("example.org.").unwrap(),
            RecordType::MX,
        ));
        socket
            .send_to(&query.to_vec().unwrap(), server.addr())
            .await
            .unwrap();
        let mut buf = [0u8; 1024];
        let (n, _) = socket.recv_from(&mut buf).await.unwrap();
        let reply = Message::from_vec(&buf[..n]).unwrap();
        assert_eq!(reply.metadata.id, query.metadata.id);
        assert_eq!(reply.answers.len(), 1);
        assert_eq!(
            server.queries(),
            [("MX".to_owned(), "example.org".to_owned())]
        );
        server.update(|z| {
            z.add_mx("example.org", 20, "backup.provider.net");
        });
        socket
            .send_to(&query.to_vec().unwrap(), server.addr())
            .await
            .unwrap();
        let (n, _) = socket.recv_from(&mut buf).await.unwrap();
        assert_eq!(Message::from_vec(&buf[..n]).unwrap().answers.len(), 2);
    }
}
