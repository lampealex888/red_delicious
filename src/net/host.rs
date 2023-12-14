use std::net::*;
use std::str::FromStr;
use bevy::math::Vec3Swizzles;
use bevy::prelude::*;
use crate::game::{Chests, player};
use crate::{menus, net};
use crate::game::buffers::{DirBuffer, EventBuffer, HpBuffer, PosBuffer};
use crate::components::*;
use crate::game::map::MapSeed;
use crate::net::packets::*;
use crate::net::{MAGIC_NUMBER, MAX_DATAGRAM_SIZE};

pub const RENDER_DISTANCE: f32 = 640.;

#[derive(Copy, Clone)]
pub struct Connection {
    pub addr: SocketAddr,
    pub player_id: u8,
    pub rmt_num: u16,  // if the ack is older than TIMEOUT ticks ago, disconnect the player
    pub ack: u32
}

#[derive(Resource)]
pub struct Connections(pub [Option<Connection>; player::MAX_PLAYERS-1]); // -1 because host not included

pub fn startup(mut commands: Commands) {
    commands.insert_resource(Connections { 0: [None; player::MAX_PLAYERS-1] });
}

pub fn connect(addresses: Res<menus::NetworkAddresses>,
    mut sock: ResMut<net::Socket>
) {
    let host_ip = Ipv4Addr::new(192, 168, 0, 1);
    let host_port = u16::from_str(&addresses.host_port).expect("bad host port");
    let host_addr = SocketAddr::new(IpAddr::from(host_ip), host_port);
    // had to be commented out unfortunately to publish to the web otherwise you get an error
    // Error { kind: Unsupported, message: "operation not supported on this platform" }
    // sock.0 = Some(UdpSocket::bind(host_addr).expect("host port in use")); 
    // sock.0.as_mut().unwrap().set_nonblocking(true).expect("can't set nonblocking");
}

pub fn disconnect(
    mut sock: ResMut<net::Socket>,
    mut conns: ResMut<Connections>
) {
    sock.0.take();
    for conn in conns.0.iter_mut() {
        conn.take();
    }
}

pub fn fixed(
    tick: Res<net::TickNum>,
    conns: Res<Connections>,
    sock: Res<net::Socket>,
    player_query: Query<(&PosBuffer, &HpBuffer, &Player, &EventBuffer, &DirBuffer, &Stats, &StoredPowerUps)>,
    enemy_query: Query<(&PosBuffer, &Health, &Enemy, &EventBuffer)>,
    powerups_query: Query<(&PowerUp, &Transform)>,
    camp_query: Query<(&Camp, &CampStatus, &CampEnemies)>,
    chests_query: Query<(&ItemChest, &Health)>
) {
    if sock.0.is_none() { return }
    let sock = sock.0.as_ref().unwrap();
    for conn in conns.0.iter() {
        if conn.is_none() { continue; }
        let conn = conn.unwrap();
        for (lp_pb, _, lp_pl, _, _, _, _) in &player_query {
            if conn.player_id == lp_pl.0 {
                // for "this" player, add self, then calculate who is close and add them.
                let lp_pos = lp_pb.0.get(tick.0);
                let mut players: Vec<PlayerTick> = Vec::new();
                for (pb, hb, pl, eb, db, stats, powerups) in &player_query {
                    let pos = pb.0.get(tick.0);
                    let hp = hb.0.get(tick.0);
                    let dir = db.0.get(tick.0);
                    let events = eb.0.get(tick.0);
                    if pos.is_none() || hp.is_none() || dir.is_none() || events.is_none() { continue }
                    let pos = pos.unwrap();
                    let hp = hp.unwrap();
                    let dir = dir.unwrap();
                    let events = events.unwrap();
                    players.push(PlayerTick {
                        id: pl.0,
                        pos,
                        dir,
                        hp,
                        events,
                        stats: stats.clone(),
                        powerups: powerups.clone(),
                    });
                }
                let mut enemies: Vec<EnemyTick> = Vec::new();
                if lp_pos.is_some() {
                    let lp_pos = lp_pos.unwrap();
                    for (pb, hp, en, eb) in &enemy_query {
                        let pos = pb.0.get(tick.0).unwrap();
                        if pos.distance(lp_pos) < RENDER_DISTANCE {
                            enemies.push(EnemyTick {
                                id: en.0,
                                pos,
                                hp: hp.current,
                                events: eb.0.get(tick.0).unwrap_or(0),
                            });
                        }
                    }
                }
                let mut powerups: Vec<(PowerUpType, Vec2)> = Vec::new();
                for (pu, pos) in &powerups_query {
                    powerups.push((pu.0, pos.translation.xy()));
                }
                let mut camps = Vec::new();
                for (camp, status, enemies) in &camp_query {
                    if status.0 {
                        camps.push((camp.0, enemies.current_enemies));
                    }
                }
                let mut chests: Vec<(u8, u8)> = Vec::new();
                for (id, hp) in &chests_query {
                    chests.push((id.id, hp.current));
                }
                let packet = HostTick {
                    seq_num: tick.0,
                    rmt_num: conn.rmt_num,
                    ack: conn.ack,
                    enemies,
                    players,
                    powerups,
                    camps,
                    chests
                };
                let peer = conn.addr;
                let mut bytes: Vec<u8> = Vec::new();
                packet.to_buf(&mut bytes);
                send_buf(bytes.as_slice(), &sock, &peer).expect(&*format!("failed to send HostTick to {:?}", peer));
            }
        }
    }
}

/// tries to find a player id given an origin
/// returns Some(player id) if successful, otherwise None
fn get_id_of_origin(conns: &Connections, origin: &SocketAddr) -> Option<u8> {
    for conn in &conns.0 {
        if conn.is_some() {
            let conn = conn.unwrap();
            if conn.addr == *origin {
                return Some(conn.player_id);
            }
        }
    }
    return None;
}

/// tries to add a connection using the given origin
/// returns Some(player id) if successful, otherwise None
fn add_connection(conns: &mut Connections, origin: &SocketAddr) -> Option<u8> {
    let mut unused = [false; player::MAX_PLAYERS - 1];
    for conn in &conns.0 {
        if let Some(conn) = conn {
            unused[conn.player_id as usize] = true;
        }
    }
    let mut fresh_id: u8 = 1;
    for (i, b) in unused.into_iter().enumerate() {
        if i == fresh_id as usize && b {
            fresh_id += 1;
        }
    }
    for conn in &mut conns.0 {
        if conn.is_none() {
            let _ = conn.insert(Connection {
                addr: *origin,
                player_id: fresh_id,
                rmt_num: 0,
                ack: 0,
            });
            return Some(fresh_id);
        }
    }
    return None;
}

pub fn update(
    mut sock: ResMut<net::Socket>,
    mut conns: ResMut<Connections>,
    tick_num: Res<net::TickNum>,
    mut usercmd_writer: EventWriter<UserCmdEvent>,
    seed: Res<MapSeed>
) {
    if sock.0.is_none() { return }
    let sock = sock.0.as_mut().unwrap();
    loop {
        let mut buf = [0; MAX_DATAGRAM_SIZE];
        if sock.peek(&mut buf).is_err() { break }
        let (_, origin) = sock.recv_from(&mut buf).unwrap();
        let magic = u16::from_be_bytes(buf[0..2].try_into().unwrap());
        if magic != MAGIC_NUMBER { break; }
        let pt = u8::from_be_bytes(buf[2..3].try_into().unwrap());
        match pt {
            pt if pt == PacketType::ConnectionRequest as u8 => {
                println!("ConnectionRequest received");
                let mut maybe_id = get_id_of_origin(&conns, &origin);
                if maybe_id.is_some() {
                    continue;  // this user is already in the server
                }
                maybe_id = add_connection(&mut conns, &origin);
                if maybe_id.is_none() {
                    send_empty_packet(PacketType::ServerFull, sock, &origin).expect("cant send server full");
                }
                let player_id = maybe_id.unwrap();
                let packet = ConnectionResponse {
                    player_id,
                    seed: seed.0
                };
                let mut bytes: Vec<u8> = Vec::new();
                packet.to_buf(&mut bytes);
                send_buf(bytes.as_slice(), sock, &origin).expect("Can't send connection response");
            },
            pt if pt == PacketType::ClientTick as u8 => {
                let packet = ClientTick::from_buf(&buf[3..]);
                if packet.is_err() {
                    println!("Malformed ClientTick Received!");
                    continue;
                }
                let packet = packet.unwrap();
                let maybe_id = get_id_of_origin(&conns, &origin);
                if maybe_id.is_none() {
                    continue;  // ignore packets from non connected clients
                }
                let id = maybe_id.unwrap();
                if packet.seq_num < tick_num.0 - net::DELAY {
                    // TODO deal with packet misses
                    println!("packet late, local is {} remote is {}", tick_num.0, packet.seq_num);
                    continue
                }
                // send event that this player has moved to this location
                usercmd_writer.send(UserCmdEvent {
                    seq_num: packet.seq_num,
                    id,
                    tick: packet.tick
                });
            },
            pt if pt == PacketType::Disconnect as u8 => {
                // TODO make player dead
                println!("disconnect received");
                for conn in &mut conns.0 {
                    if conn.is_some() {
                        let s = conn.unwrap().addr;
                        if s == origin {
                            conn.take();
                        }
                    }
                }
            }
            _ => panic!("Bad packet sent to host")
        }
    }
}
