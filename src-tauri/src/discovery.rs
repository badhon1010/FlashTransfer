use crate::types::{DeviceInfo, DeviceKind};
use log::{error, info, warn};
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use std::net::IpAddr;
use tauri::{AppHandle, Emitter};

const SERVICE_TYPE: &str = "_flashtransfer._tcp.local.";

/// Detect the primary local IPv4 address by routing a dummy UDP packet.
/// Does not send any actual network traffic.
pub fn get_local_ip() -> Option<String> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    socket.local_addr().ok().map(|a| a.ip().to_string())
}

/// Get the OS hostname, falling back to "FlashTransfer-Device".
pub fn get_hostname() -> String {
    hostname::get()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_else(|_| "FlashTransfer-Device".to_string())
}

#[cfg(target_os = "macos")]
fn native_device_kind() -> DeviceKind { DeviceKind::Laptop }
#[cfg(target_os = "windows")]
fn native_device_kind() -> DeviceKind { DeviceKind::Desktop }
#[cfg(target_os = "linux")]
fn native_device_kind() -> DeviceKind { DeviceKind::Desktop }
#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
fn native_device_kind() -> DeviceKind { DeviceKind::Unknown }

fn kind_to_str(kind: &DeviceKind) -> &'static str {
    match kind {
        DeviceKind::Laptop  => "laptop",
        DeviceKind::Desktop => "desktop",
        DeviceKind::Phone   => "phone",
        DeviceKind::Tablet  => "tablet",
        DeviceKind::Unknown => "unknown",
    }
}

fn str_to_kind(s: &str) -> DeviceKind {
    match s {
        "laptop"  => DeviceKind::Laptop,
        "desktop" => DeviceKind::Desktop,
        "phone"   => DeviceKind::Phone,
        "tablet"  => DeviceKind::Tablet,
        _         => DeviceKind::Unknown,
    }
}

/// Spawn a background thread that:
/// 1. Registers this device on the local mDNS network.
/// 2. Continuously browses for other FlashTransfer devices.
/// 3. Emits `device-discovered` / `device-removed` Tauri events.
///
/// The `mdns` daemon must live as long as discovery is needed — pass the one
/// stored in `AppState` so it isn't dropped.
pub fn start_discovery(app: AppHandle, mdns: ServiceDaemon, port: u16, hostname: String) {
    std::thread::spawn(move || {
        run_discovery(app, mdns, port, hostname);
    });
}

fn run_discovery(app: AppHandle, mdns: ServiceDaemon, port: u16, hostname: String) {
    // mDNS instance names must not contain spaces; replace with hyphens.
    let instance = hostname.replace(' ', "-");

    let local_ip_str = match get_local_ip() {
        Some(ip) => ip,
        None => {
            error!("discovery: could not determine local IP; aborting");
            return;
        }
    };

    let local_ip: IpAddr = match local_ip_str.parse() {
        Ok(ip) => ip,
        Err(e) => {
            error!("discovery: invalid local IP '{}': {e}", local_ip_str);
            return;
        }
    };

    let host_name = format!("{instance}.local.");
    let kind_str  = kind_to_str(&native_device_kind());

    // Build TXT properties map
    let props: std::collections::HashMap<String, String> =
        [("kind".to_string(), kind_str.to_string())].into();

    match ServiceInfo::new(SERVICE_TYPE, &instance, &host_name, local_ip, port, Some(props)) {
        Ok(svc) => match mdns.register(svc) {
            Ok(()) => info!("discovery: registered '{instance}' at {local_ip_str}:{port}"),
            Err(e) => warn!("discovery: register failed: {e}"),
        },
        Err(e) => warn!("discovery: ServiceInfo::new failed: {e}"),
    }

    let receiver = match mdns.browse(SERVICE_TYPE) {
        Ok(r) => r,
        Err(e) => {
            error!("discovery: browse failed: {e}");
            return;
        }
    };

    loop {
        match receiver.recv() {
            Ok(event) => handle_event(&app, &instance, event),
            Err(e) => {
                error!("discovery: receiver closed: {e}");
                break;
            }
        }
    }
}

fn handle_event(app: &AppHandle, local_instance: &str, event: ServiceEvent) {
    match event {
        ServiceEvent::ServiceResolved(info) => {
            // Strip service-type suffix to get the plain instance name.
            let full = info.get_fullname();
            let name = full
                .strip_suffix(&format!(".{SERVICE_TYPE}"))
                .or_else(|| full.strip_suffix(&format!(".{}", SERVICE_TYPE.trim_end_matches('.'))))
                .unwrap_or(full)
                .to_string();

            // Ignore ourselves.
            if name == local_instance { return; }

            // Prefer IPv4 addresses.
            let ip = info
                .get_addresses()
                .iter()
                .find(|a| a.is_ipv4())
                .map(std::string::ToString::to_string)
                .unwrap_or_default();

            let port = info.get_port();

            let kind = info
                .get_properties()
                .get("kind")
                .map(|v| str_to_kind(v.val_str()))
                .unwrap_or(DeviceKind::Unknown);

            let device = DeviceInfo {
                id:   full.to_string(),
                name: name.replace('-', " "),
                kind,
                ip,
                port,
            };

            info!("discovery: found {:?}", device.name);

            if let Err(e) = app.emit("device-discovered", &device) {
                error!("discovery: emit device-discovered failed: {e}");
            }
        }
        ServiceEvent::ServiceRemoved(_, full_name) => {
            info!("discovery: removed {full_name}");
            if let Err(e) = app.emit("device-removed", &full_name) {
                error!("discovery: emit device-removed failed: {e}");
            }
        }
        _ => {}
    }
}
