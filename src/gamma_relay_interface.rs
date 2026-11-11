use crate::message::GammaRelayProperty;

#[zbus::proxy(
    interface = "rs.wl.gammarelay",
    default_service = "rs.wl-gammarelay",
    default_path = "/"
)]
pub trait GammaRelayInterface {
    #[zbus(property)]
    fn temperature(&self) -> zbus::Result<u16>;
    #[zbus(property)]
    fn set_temperature(&self, value: u16) -> zbus::Result<()>;

    #[zbus(property)]
    fn gamma(&self) -> zbus::Result<f64>;
    #[zbus(property)]
    fn set_gamma(&self, value: f64) -> zbus::Result<()>;

    #[zbus(property)]
    fn brightness(&self) -> zbus::Result<f64>;
    #[zbus(property)]
    fn set_brightness(&self, value: f64) -> zbus::Result<()>;
}

pub async fn set_property<'a>(
    gamma_relay_prox: &GammaRelayInterfaceProxy<'a>,
    property: &GammaRelayProperty,
    value: f64,
) -> zbus::Result<()> {
    match property {
        GammaRelayProperty::Temperature => gamma_relay_prox.set_temperature(value as u16).await,
        GammaRelayProperty::Brightness => gamma_relay_prox.set_brightness(value).await,
        GammaRelayProperty::Gamma => gamma_relay_prox.set_gamma(value).await,
    }
}
pub async fn get_property<'a>(
    gamma_relay_prox: &GammaRelayInterfaceProxy<'a>,
    property: &GammaRelayProperty,
) -> zbus::Result<f64> {
    match property {
        GammaRelayProperty::Temperature => gamma_relay_prox.temperature().await.map(|t| t as f64),
        GammaRelayProperty::Brightness => gamma_relay_prox.brightness().await,
        GammaRelayProperty::Gamma => gamma_relay_prox.gamma().await,
    }
}
