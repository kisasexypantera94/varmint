use vmnet::{
    Error, Events, Interface, Result,
    parameters::{Parameter, ParameterKind},
};

pub struct Backend {
    iface: Interface,
}

unsafe impl Send for Backend {}

impl Backend {
    pub fn new() -> Result<Backend> {
        let iface = vmnet::Interface::new(
            vmnet::mode::Mode::Shared(vmnet::mode::Shared::default()),
            vmnet::Options::default(),
        )?;

        let params = iface.parameters();
        if let Some(Parameter::HostIPAddress(ip)) = params.get(ParameterKind::HostIPAddress) {
            eprintln!("vmnet host IP: {}", ip);
        }
        if let Some(Parameter::SubnetMask(mask)) = params.get(ParameterKind::SubnetMask) {
            eprintln!("vmnet subnet mask: {}", mask);
        }
        if let Some(Parameter::StartAddress(ip)) = params.get(ParameterKind::StartAddress) {
            eprintln!("vmnet DHCP start: {}", ip);
        }

        Ok(Backend { iface })
    }

    pub fn mac(&self) -> [u8; 6] {
        if let Some(Parameter::MACAddress(mac_str)) = self.iface.parameters().get(ParameterKind::MACAddress) {
            let bytes: Vec<u8> = mac_str.split(':').map(|s| u8::from_str_radix(s, 16).unwrap()).collect();
            bytes.try_into().unwrap()
        } else {
            panic!("vmnet did not provide MAC address");
        }
    }

    pub fn max_packet_size(&self) -> u64 {
        if let Some(Parameter::MaxPacketSize(max_packet_size)) =
            self.iface.parameters().get(ParameterKind::MaxPacketSize)
        {
            max_packet_size
        } else {
            panic!("vmnet did not provide MAC address");
        }
    }

    pub fn set_event_callback<F: Fn() + 'static>(&mut self, cb: F) -> Result<()> {
        self.iface
            .set_event_callback(Events::PACKETS_AVAILABLE, move |_, params| {
                if let Some(Parameter::EstimatedPacketsAvailable(_)) =
                    params.get(ParameterKind::EstimatedPacketsAvailable)
                {
                    cb();
                }
            })
    }

    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        let res = self.iface.read(buf);

        if matches!(res, Err(Error::VmnetReadNothing)) {
            Ok(0)
        } else {
            res
        }
    }

    pub fn write(&mut self, buf: &[u8]) -> Result<usize> {
        self.iface.write(buf)
    }
}
