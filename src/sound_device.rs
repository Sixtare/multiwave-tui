use cpal::{Device, StreamConfig, traits::{DeviceTrait, HostTrait}};


pub struct SoundDevice{
    name: String,
    device: Device,
    info: String,
}

impl SoundDevice {
    pub fn new() -> Self{
        let host = cpal::default_host();
        let device = host.default_output_device().expect("Not output device detected");

        let config: StreamConfig = device.default_input_config()
            .expect("Error obtaining input stream config")
            .into();

        let info = String::from("Audio configs loaded: sample rate: {config.sample_rate}, channels: {config.channels}");

        let device_name = device.id().unwrap().to_string();
        SoundDevice { name: device_name, device, info }
    }
}