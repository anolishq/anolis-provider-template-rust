//! The provider: simulated heaters, no hardware.
//!
//! Start here when writing a provider. Each configured device is a heater with
//! a first-order thermal model: its temperature moves towards ambient plus the
//! rise its power level gives, with a fixed time constant, and it counts the
//! energy it has drawn. Replace the model with calls into your device's driver
//! and keep the rest of the shape:
//!
//! - `HEATER` declares the device type: what the runtime can read and call.
//! - `Config` declares the config; `check_config` refuses what its schema
//!   cannot say.
//! - `read` and `call` serve requests; `stop` puts every output in its safe
//!   state however the provider exits.

use std::collections::HashSet;
use std::process::ExitCode;
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};

use anolis_provider_sdk::{
    Arg, Args, BoxError, CallContext, Category, Device, DeviceType, Function, Provider, ProviderError, Readings,
    Results, Signal, Value, ValueType,
};
use schemars::JsonSchema;
use serde::Deserialize;

const TIME_CONSTANT: Duration = Duration::from_secs(60);

static HEATER: LazyLock<Arc<DeviceType>> = LazyLock::new(|| {
    DeviceType::new("heater")
        .signal(Signal::new("temperature", ValueType::Double).unit("degC").description("Simulated temperature"))
        .signal(Signal::new("power", ValueType::Double).description("Power level, 0 (off) to 1 (full)"))
        .signal(
            Signal::new("energy", ValueType::Double)
                .unit("J")
                .description("Energy drawn since the last reset")
                .not_default(),
        )
        // ACTUATE with a required numeric argument whose 0 is safe: the
        // runtime's zero_is_safe rung can then stop it by calling it with 0.
        .function(
            Function::new("set_power")
                .arg(Arg::double("power").range(0.0, 1.0).description("0 (off) to 1 (full)"))
                .description("Set the power level; 0 turns the heater off")
                .category(Category::Actuate)
                .idempotent(),
        )
        .function(
            Function::new("reset_energy")
                .result(Arg::double("energy").unit("J").description("The energy count before the reset"))
                .description("Set the energy count to zero")
                .category(Category::Config),
        )
        .build()
});

/// anolis-provider-template-rust config
#[derive(Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Config {
    #[schemars(length(min = 1))]
    devices: Vec<HeaterConfig>,
}

#[derive(Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct HeaterConfig {
    /// Device id
    #[schemars(regex(pattern = r"^[a-z][a-z0-9_]*$"))]
    id: String,
    label: Option<String>,
    /// Ambient temperature, degC
    #[serde(default = "default_ambient_c")]
    ambient_c: f64,
    /// Temperature rise above ambient at full power, degC
    #[serde(default = "default_max_rise_c")]
    #[schemars(extend("exclusiveMinimum" = 0))]
    max_rise_c: f64,
    /// Power drawn at full power, W
    #[serde(default = "default_rated_w")]
    #[schemars(extend("exclusiveMinimum" = 0))]
    rated_w: f64,
}

fn default_ambient_c() -> f64 {
    20.0
}
fn default_max_rise_c() -> f64 {
    50.0
}
fn default_rated_w() -> f64 {
    100.0
}

/// The simulated device. Its state advances to the time it is read or set at.
struct Heater {
    ambient_c: f64,
    max_rise_c: f64,
    rated_w: f64,
    temperature: f64,
    power: f64,
    energy: f64,
    at: Instant,
}

impl Heater {
    fn new(config: &HeaterConfig, now: Instant) -> Self {
        Self {
            ambient_c: config.ambient_c,
            max_rise_c: config.max_rise_c,
            rated_w: config.rated_w,
            temperature: config.ambient_c,
            power: 0.0,
            energy: 0.0,
            at: now,
        }
    }

    fn advance(&mut self, now: Instant) {
        let dt = now.saturating_duration_since(self.at).as_secs_f64();
        self.at = now;
        let target = self.ambient_c + self.power * self.max_rise_c;
        self.temperature = target + (self.temperature - target) * (-dt / TIME_CONSTANT.as_secs_f64()).exp();
        self.energy += self.power * self.rated_w * dt;
    }
}

struct TemplateProvider {
    devices: Vec<Device>,
    heaters: Vec<Heater>,
}

impl TemplateProvider {
    fn heater(&mut self, device: &Device) -> &mut Heater {
        let index = self.devices.iter().position(|d| d.id() == device.id()).expect("the SDK passes known devices");
        let heater = &mut self.heaters[index];
        heater.advance(Instant::now());
        heater
    }
}

impl Provider for TemplateProvider {
    type Config = Config;
    const NAME: &'static str = env!("CARGO_PKG_NAME");
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");
    const CONFORMANCE_LEVEL: Option<u32> = Some(2);

    /// What the schema cannot say: each device id once.
    fn check_config(config: &Config) -> Result<(), BoxError> {
        let mut seen = HashSet::new();
        for heater in &config.devices {
            if !seen.insert(&heater.id) {
                return Err(format!("device id {:?} is configured twice", heater.id).into());
            }
        }
        Ok(())
    }

    fn new(config: Config) -> Result<Self, BoxError> {
        let now = Instant::now();
        let devices = config
            .devices
            .iter()
            .map(|h| Device::new(&h.id, &HEATER).label(h.label.as_ref().unwrap_or(&h.id)))
            .collect();
        let heaters = config.devices.iter().map(|h| Heater::new(h, now)).collect();
        Ok(Self { devices, heaters })
    }

    fn devices(&self) -> &[Device] {
        &self.devices
    }

    fn read(&mut self, device: &Device, signal_ids: &[&str]) -> Result<Readings, ProviderError> {
        let heater = self.heater(device);
        let value = |id: &str| match id {
            "temperature" => heater.temperature,
            "power" => heater.power,
            _ => heater.energy,
        };
        Ok(signal_ids.iter().map(|&id| (id.to_owned(), value(id).into())).collect())
    }

    fn call(
        &mut self,
        device: &Device,
        function: &Function,
        args: &Args,
        _ctx: &CallContext,
    ) -> Result<Results, ProviderError> {
        let heater = self.heater(device);
        match function.name() {
            // The SDK has already checked power is present and within 0..1.
            "set_power" => {
                heater.power = args.f64("power").expect("a required argument");
                Ok(Results::new())
            }
            "reset_energy" => {
                let energy = std::mem::take(&mut heater.energy);
                Ok(Results::from([("energy".to_owned(), Value::from(energy))]))
            }
            other => unreachable!("undeclared function {other}"), // the SDK only passes declared ones
        }
    }

    /// The provider is exiting, however the session ended: turn every heater off.
    fn stop(&mut self) {
        let now = Instant::now();
        for heater in &mut self.heaters {
            heater.advance(now);
            heater.power = 0.0;
        }
    }
}

fn main() -> ExitCode {
    anolis_provider_sdk::run::<TemplateProvider>()
}

#[cfg(test)]
mod tests {
    //! The heater model and the provider's own rules. The protocol itself is
    //! covered by the conformance harness (scripts/conformance.py).

    use super::*;

    fn config(ambient_c: f64, max_rise_c: f64, rated_w: f64) -> HeaterConfig {
        HeaterConfig { id: "h0".into(), label: None, ambient_c, max_rise_c, rated_w }
    }

    #[test]
    fn a_heater_starts_off_at_ambient() {
        let t0 = Instant::now();
        let mut h = Heater::new(&config(21.0, 50.0, 100.0), t0);
        h.advance(t0 + Duration::from_secs(600));
        assert_eq!((h.temperature, h.power, h.energy), (21.0, 0.0, 0.0));
    }

    #[test]
    fn full_power_approaches_ambient_plus_the_rise() {
        let t0 = Instant::now();
        let mut h = Heater::new(&config(20.0, 50.0, 100.0), t0);
        h.power = 1.0;
        h.advance(t0 + TIME_CONSTANT);
        assert!((h.temperature - (20.0 + 50.0 * (1.0 - (-1.0f64).exp()))).abs() < 1e-9, "{}", h.temperature);
        h.advance(t0 + TIME_CONSTANT * 21);
        assert!((h.temperature - 70.0).abs() < 0.01, "{}", h.temperature);
    }

    #[test]
    fn energy_counts_power_times_rating() {
        let t0 = Instant::now();
        let mut h = Heater::new(&config(20.0, 50.0, 60.0), t0);
        h.power = 0.5;
        h.advance(t0 + Duration::from_secs(10));
        assert!((h.energy - 300.0).abs() < 1e-9, "{}", h.energy);
    }

    #[test]
    fn stop_turns_every_heater_off() {
        let mut p = TemplateProvider::new(Config { devices: vec![config(20.0, 50.0, 100.0)] }).unwrap();
        p.heaters[0].power = 1.0;
        p.stop();
        assert_eq!(p.heaters[0].power, 0.0);
    }

    #[test]
    fn a_device_id_configured_twice_is_refused() {
        let twice = Config { devices: vec![config(20.0, 50.0, 100.0), config(20.0, 50.0, 100.0)] };
        let err = TemplateProvider::check_config(&twice).unwrap_err();
        assert!(err.to_string().contains("configured twice"), "{err}");
    }
}
