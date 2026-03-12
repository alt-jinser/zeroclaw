use clap::Subcommand;
use serde::{Deserialize, Serialize};

#[derive(clap::Args, Debug)]
pub(crate) struct HardwareArgs {
    #[command(subcommand)]
    hardware_command: HardwareCommands,
}

/// Hardware discovery subcommands
#[derive(Subcommand, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
enum HardwareCommands {
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

pub(crate) fn run(args: HardwareArgs) -> anyhow::Result<()> {
    let cmd = args.hardware_command;

    #[cfg(not(feature = "hardware"))]
    {
        let _ = &cmd;
        println!("Hardware discovery requires the 'hardware' feature.");
        println!("Build with: cargo build --features hardware");
        Ok(())
    }

    #[cfg(all(
        feature = "hardware",
        not(any(target_os = "linux", target_os = "macos", target_os = "windows"))
    ))]
    {
        let _ = &cmd;
        println!("Hardware USB discovery is not supported on this platform.");
        println!("Supported platforms: Linux, macOS, Windows.");
        return Ok(());
    }

    // TODO: fix use
    #[cfg(all(
        feature = "hardware",
        any(target_os = "linux", target_os = "macos", target_os = "windows")
    ))]
    match cmd {
        HardwareCommands::Discover => run_discover(),
        HardwareCommands::Introspect { path } => run_introspect(&path),
        HardwareCommands::Info { chip } => run_info(&chip),
    }
}
