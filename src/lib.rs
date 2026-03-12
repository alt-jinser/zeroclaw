#![forbid(unsafe_code)]
#![recursion_limit = "256"]

use clap::Subcommand;
use serde::{Deserialize, Serialize};

pub mod agent;
pub mod approval;
pub mod auth;
pub mod channels;
pub mod config;
pub mod coordination;
pub mod cost;
pub mod cron;
pub mod daemon;
pub mod doctor;
pub mod economic;
pub mod gateway;
pub mod goals;
pub mod hardware;
pub mod health;
pub mod heartbeat;
pub mod hooks;
pub mod identity;
pub mod integrations;
pub mod memory;
pub mod migration;
pub mod multimodal;
pub mod observability;
pub mod onboard;
pub mod peripherals;
pub mod plugins;
pub mod providers;
pub mod rag;
pub mod runtime;
pub mod security;
pub mod service;
pub mod skillforge;
pub mod skills;
#[cfg(test)]
pub mod test_locks;
pub mod tools;
pub mod tunnel;
pub mod update;
pub mod util;

pub use config::Config;

pub const ZEROCLAW_BUILD_VERSION: &str = env!("ZEROCLAW_BUILD_VERSION");

/// Memory management subcommands
#[derive(Subcommand, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MemoryCommands {
    /// List memory entries with optional filters
    List {
        /// Filter by category (core, daily, conversation, or custom name)
        #[arg(long)]
        category: Option<String>,
        /// Filter by session ID
        #[arg(long)]
        session: Option<String>,
        /// Maximum number of entries to display
        #[arg(long, default_value = "50")]
        limit: usize,
        /// Number of entries to skip (for pagination)
        #[arg(long, default_value = "0")]
        offset: usize,
    },
    /// Get a specific memory entry by key
    Get {
        /// Memory key to look up
        key: String,
    },
    /// Show memory backend statistics and health
    Stats,
    /// Clear memories by category, by key, or clear all
    Clear {
        /// Delete a single entry by key (supports prefix match)
        #[arg(long)]
        key: Option<String>,
        /// Only clear entries in this category
        #[arg(long)]
        category: Option<String>,
        /// Skip confirmation prompt
        #[arg(long)]
        yes: bool,
    },
    /// Rebuild embeddings for all memories (use after changing embedding model)
    Reindex {
        /// Skip confirmation prompt
        #[arg(long)]
        yes: bool,
        /// Show progress during reindex
        #[arg(long, default_value = "true")]
        progress: bool,
    },
}

/// Hardware discovery subcommands
#[derive(Subcommand, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum HardwareCommands {
    /// Enumerate USB devices (VID/PID) and show known boards
    #[command(long_about = "\
Enumerate USB devices and show known boards.

Scans connected USB devices by VID/PID and matches them against \
known development boards (STM32 Nucleo, Arduino, ESP32).

Examples:
  zeroclaw hardware discover")]
    Discover,
    /// Introspect a device by path (e.g. /dev/ttyACM0)
    #[command(long_about = "\
Introspect a device by its serial or device path.

Opens the specified device path and queries for board information, \
firmware version, and supported capabilities.

Examples:
  zeroclaw hardware introspect /dev/ttyACM0
  zeroclaw hardware introspect COM3")]
    Introspect {
        /// Serial or device path
        path: String,
    },
    /// Get chip info via USB (probe-rs over ST-Link). No firmware needed on target.
    #[command(long_about = "\
Get chip info via USB using probe-rs over ST-Link.

Queries the target MCU directly through the debug probe without \
requiring any firmware on the target board.

Examples:
  zeroclaw hardware info
  zeroclaw hardware info --chip STM32F401RETx")]
    Info {
        /// Chip name (e.g. STM32F401RETx). Default: STM32F401RETx for Nucleo-F401RE
        #[arg(long, default_value = "STM32F401RETx")]
        chip: String,
    },
}

/// Peripheral (hardware) management subcommands
#[derive(Subcommand, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PeripheralCommands {
    /// List configured peripherals
    List,
    /// Add a peripheral (board path, e.g. nucleo-f401re /dev/ttyACM0)
    #[command(long_about = "\
Add a peripheral by board type and transport path.

Registers a hardware board so the agent can use its tools (GPIO, \
sensors, actuators). Use 'native' as path for local GPIO on \
single-board computers like Raspberry Pi.

Supported boards: nucleo-f401re, rpi-gpio, esp32, arduino-uno.

Examples:
  zeroclaw peripheral add nucleo-f401re /dev/ttyACM0
  zeroclaw peripheral add rpi-gpio native
  zeroclaw peripheral add esp32 /dev/ttyUSB0")]
    Add {
        /// Board type (nucleo-f401re, rpi-gpio, esp32)
        board: String,
        /// Path for serial transport (/dev/ttyACM0) or "native" for local GPIO
        path: String,
    },
    /// Flash ZeroClaw firmware to Arduino (creates .ino, installs arduino-cli if needed, uploads)
    #[command(long_about = "\
Flash ZeroClaw firmware to an Arduino board.

Generates the .ino sketch, installs arduino-cli if it is not \
already available, compiles, and uploads the firmware.

Examples:
  zeroclaw peripheral flash
  zeroclaw peripheral flash --port /dev/cu.usbmodem12345
  zeroclaw peripheral flash -p COM3")]
    Flash {
        /// Serial port (e.g. /dev/cu.usbmodem12345). If omitted, uses first arduino-uno from config.
        #[arg(short, long)]
        port: Option<String>,
    },
    /// Setup Arduino Uno Q Bridge app (deploy GPIO bridge for agent control)
    SetupUnoQ {
        /// Uno Q IP (e.g. 192.168.0.48). If omitted, assumes running ON the Uno Q.
        #[arg(long)]
        host: Option<String>,
    },
    /// Flash ZeroClaw firmware to Nucleo-F401RE (builds + probe-rs run)
    FlashNucleo,
}
