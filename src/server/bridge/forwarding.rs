use std::{fmt::Write, net::SocketAddr};

use serde::Deserialize;
use tokio::net::TcpStream;

use crate::{
    protocol::{lazy::DecodedPacket, packet, packets::Handshake, uuid::PlayerUuid},
    HopperError,
};

#[derive(Debug, Default, Deserialize, Clone, Copy)]
pub enum ForwardStrategy {
    #[default]
    #[serde(rename = "none")]
    None,

    #[serde(rename = "bungeecord")]
    BungeeCord,

    // RealIP <=2.4 support
    #[serde(rename = "realip")]
    RealIP,
}

#[async_trait::async_trait]
pub trait ConnectionPrimer {
    /// method for priming the connection of a server
    /// which may be with address forwarding informations
    /// or not, up to the implementer
    ///
    /// `og_handshake` is the original handshake that was sent to hoppper
    /// by the client
    async fn prime_connection(
        self,
        stream: &mut TcpStream,
        og_handshake: DecodedPacket<Handshake>,
    ) -> Result<(), HopperError>;
}

pub(super) struct BungeeCord {
    player_addr: SocketAddr,
    player_uuid: PlayerUuid,
}

impl BungeeCord {
    pub fn from_username(player_addr: SocketAddr, player_name: &str) -> Self {
        Self {
            player_addr,
            // calculate the player's offline UUID. It will get
            // ignored by online-mode servers so we can always send
            // it even when the server is premium-only
            player_uuid: PlayerUuid::offline_player(player_name),
        }
    }
}

#[async_trait::async_trait]
impl ConnectionPrimer for BungeeCord {
    async fn prime_connection(
        self,
        stream: &mut TcpStream,
        og_handshake: DecodedPacket<Handshake>,
    ) -> Result<(), HopperError> {
        let mut handshake = og_handshake.into_data();

        // if handshake contains a null character it means that
        // someone is trying to hijack the connection or trying to
        // connect through another proxy
        if handshake.server_address.contains('\x00') {
            return Err(HopperError::Invalid);
        }

        // https://github.com/SpigotMC/BungeeCord/blob/8d494242265790df1dc6d92121d1a37b726ac405/proxy/src/main/java/net/md_5/bungee/ServerConnector.java#L91-L106
        write!(
            handshake.server_address,
            "\x00{}\x00{}",
            self.player_addr.ip(),
            self.player_uuid
        )
        .unwrap();

        // send the modified handshake
        packet::write_serialize(handshake, stream).await?;

        Ok(())
    }
}

pub struct RealIP {
    player_addr: SocketAddr,
}

impl RealIP {
    pub fn new(player_addr: SocketAddr) -> Self {
        Self { player_addr }
    }
}

#[async_trait::async_trait]
impl ConnectionPrimer for RealIP {
    async fn prime_connection(
        self,
        stream: &mut TcpStream,
        og_handshake: DecodedPacket<Handshake>,
    ) -> Result<(), HopperError> {
        let mut handshake = og_handshake.into_data();

        // if the original handshake contains these character
        // the client is trying to hijack realip
        if handshake.server_address.contains('/') {
            return Err(HopperError::Invalid);
        }

        // FML support
        let insert_index = handshake
            .server_address
            .find('\x00')
            .map(|a| a - 1)
            .unwrap_or(handshake.server_address.len());

        // bungeecord and realip forwarding have a very similar structure
        // write!(handshake.server_address, "///{}", client.address).unwrap();
        let realip_data = format!("///{}", self.player_addr);
        handshake
            .server_address
            .insert_str(insert_index, &realip_data);

        // server.write_serialize(handshake).await?;
        packet::write_serialize(handshake, stream).await?;

        Ok(())
    }
}

/// Passthrough primer, does not modify the original
/// handshake and just sends along bytes as-is
pub(super) struct Passthrough;

#[async_trait::async_trait]
impl ConnectionPrimer for Passthrough {
    async fn prime_connection(
        self,
        stream: &mut TcpStream,
        og_handshake: DecodedPacket<Handshake>,
    ) -> Result<(), HopperError> {
        // just send along without doing anything
        og_handshake.as_ref().write_into(stream).await?;
        Ok(())
    }
}
