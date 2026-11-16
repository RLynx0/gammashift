use std::{sync::Mutex, time::Duration};

use chrono::Local;
use tokio::{sync::mpsc, task::AbortHandle, time::timeout};

use super::*;
use GammaRelayProperty::{Brightness, Temperature};
use TargetValue::{Absolute, Linear, Relative, Undo};

struct TestRelay {
    values: Arc<Mutex<(u16, f64)>>,
    writes: mpsc::UnboundedSender<(GammaRelayProperty, f64)>,
}

#[zbus::interface(name = "rs.wl.gammarelay")]
impl TestRelay {
    #[zbus(property)]
    fn temperature(&self) -> u16 {
        self.values.lock().unwrap().0
    }

    #[zbus(property)]
    fn set_temperature(&mut self, value: u16) {
        self.values.lock().unwrap().0 = value;
        let _ = self.writes.send((Temperature, f64::from(value)));
    }

    #[zbus(property)]
    fn brightness(&self) -> f64 {
        self.values.lock().unwrap().1
    }

    #[zbus(property)]
    fn set_brightness(&mut self, value: f64) {
        self.values.lock().unwrap().1 = value;
        let _ = self.writes.send((Brightness, value));
    }
}

struct Fixture {
    coordinator: Coordinator,
    values: Arc<Mutex<(u16, f64)>>,
    written: mpsc::UnboundedReceiver<(GammaRelayProperty, f64)>,
    server: zbus::Connection,
}

impl Fixture {
    async fn new() -> Self {
        let values = Arc::new(Mutex::new((4000, 1.0)));
        let (writes, written) = mpsc::unbounded_channel();
        let server = zbus::connection::Builder::session()
            .serve_at(
                "/",
                TestRelay {
                    values: values.clone(),
                    writes,
                },
            )
            .build()
            .await
            .unwrap();
        let connection = zbus::Connection::session().await.unwrap();
        let proxy = GammaRelayInterfaceProxy::builder(&connection)
            .destination(server.unique_name().unwrap().to_owned())
            .cache_properties(zbus::proxy::CacheProperties::No)
            .build()
            .await
            .unwrap();
        Self {
            coordinator: Coordinator {
                proxy,
                in_flight: HashMap::new(),
                history: HashMap::new(),
            },
            values,
            written,
            server,
        }
    }

    async fn shift(&mut self, property: GammaRelayProperty, target: TargetValue, id: &str) -> f64 {
        self.coordinator
            .shift(message(property, target, id))
            .await
            .unwrap();
        let (written_property, value) = self.written.recv().await.unwrap();
        assert_eq!(written_property, property);
        value
    }

    fn handle(&self, property: GammaRelayProperty) -> AbortHandle {
        self.coordinator.in_flight[&property].handle.abort_handle()
    }

    fn record(&self, property: GammaRelayProperty, id: &str) -> ShiftRecord {
        self.coordinator.history[&ShiftId {
            property,
            id: Arc::from(id),
        }]
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        for in_flight in self.coordinator.in_flight.values() {
            in_flight.handle.abort();
        }
    }
}

async fn with_coordinator(test: impl AsyncFnOnce(&mut Fixture)) {
    timeout(Duration::from_secs(10), async {
        let mut fixture = Fixture::new().await;
        test(&mut fixture).await;
        fixture.coordinator.stop().await;
    })
    .await
    .expect("coordinator test timed out");
}

fn message(property: GammaRelayProperty, target: TargetValue, id: &str) -> ShiftMessage {
    let start = Local::now();
    ShiftMessage {
        property,
        id: id.into(),
        target,
        step_duration: Duration::from_secs(3600),
        start,
        end: start + Duration::from_secs(3600),
    }
}

#[tokio::test]
#[ignore = "requires a session bus: dbus-run-session -- cargo test -- --include-ignored"]
async fn shift_cancels_the_previous_ramp_with_the_same_id() {
    with_coordinator(async |fixture| {
        fixture
            .shift(Temperature, Absolute(4500.0), "example")
            .await;
        let previous = fixture.handle(Temperature);

        fixture
            .shift(Temperature, Absolute(5000.0), "example")
            .await;

        assert!(previous.is_finished());
        assert!(!fixture.handle(Temperature).is_finished());
    })
    .await;
}

#[tokio::test]
#[ignore = "requires a session bus: dbus-run-session -- cargo test -- --include-ignored"]
async fn shift_cancels_the_previous_ramp_with_a_different_id() {
    with_coordinator(async |fixture| {
        fixture.shift(Temperature, Absolute(4500.0), "first").await;
        let previous = fixture.handle(Temperature);

        fixture.shift(Temperature, Absolute(5000.0), "second").await;

        assert!(previous.is_finished());
        assert!(!fixture.handle(Temperature).is_finished());
    })
    .await;
}

#[tokio::test]
#[ignore = "requires a session bus: dbus-run-session -- cargo test -- --include-ignored"]
async fn shift_does_not_cancel_another_property() {
    with_coordinator(async |fixture| {
        fixture.shift(Brightness, Absolute(0.5), "example").await;
        let brightness = fixture.handle(Brightness);

        fixture
            .shift(Temperature, Absolute(4500.0), "example")
            .await;

        assert!(!brightness.is_finished());
        assert!(!fixture.handle(Temperature).is_finished());
    })
    .await;
}

#[tokio::test]
#[ignore = "requires a session bus: dbus-run-session -- cargo test -- --include-ignored"]
async fn relative_shift_uses_the_previous_target_but_ramps_from_actual() {
    with_coordinator(async |fixture| {
        fixture.shift(Temperature, Relative(500.0), "first").await;
        fixture.values.lock().unwrap().0 = 4100;

        let first_write = fixture.shift(Temperature, Relative(500.0), "second").await;

        assert!(
            (first_write - 4100.0).abs() < 2.0,
            "ramp must start near actual"
        );
        assert_eq!(
            fixture.record(Temperature, "second"),
            ShiftRecord {
                reference: 4500.0,
                target: 5000.0
            }
        );
    })
    .await;
}

#[tokio::test]
#[ignore = "requires a session bus: dbus-run-session -- cargo test -- --include-ignored"]
async fn linear_shift_uses_the_previous_target_but_ramps_from_actual() {
    with_coordinator(async |fixture| {
        fixture
            .shift(Temperature, Absolute(4500.0), "example")
            .await;
        fixture.values.lock().unwrap().0 = 4100;

        let first_write = fixture.shift(Temperature, Linear(1.5), "example").await;

        assert!(
            (first_write - 4100.0).abs() < 2.0,
            "ramp must start near actual"
        );
        assert_eq!(
            fixture.record(Temperature, "example"),
            ShiftRecord {
                reference: 4500.0,
                target: 6750.0
            }
        );
    })
    .await;
}

#[tokio::test]
#[ignore = "requires a session bus: dbus-run-session -- cargo test -- --include-ignored"]
async fn repeated_undo_toggles_between_the_previous_reference_and_target() {
    with_coordinator(async |fixture| {
        fixture.shift(Temperature, Relative(500.0), "example").await;
        fixture.shift(Temperature, Relative(500.0), "example").await;

        for (reference, target) in [(5000.0, 4500.0), (4500.0, 5000.0), (5000.0, 4500.0)] {
            fixture.values.lock().unwrap().0 = 4200;

            let first_write = fixture.shift(Temperature, Undo, "example").await;

            assert!(
                (first_write - 4200.0).abs() < 2.0,
                "undo must ramp from actual"
            );
            assert_eq!(
                fixture.record(Temperature, "example"),
                ShiftRecord { reference, target }
            );
        }
    })
    .await;
}

#[tokio::test]
#[ignore = "requires a session bus: dbus-run-session -- cargo test -- --include-ignored"]
async fn undo_without_history_preserves_the_running_shift_and_history() {
    with_coordinator(async |fixture| {
        fixture
            .shift(Temperature, Absolute(4500.0), "example")
            .await;
        let previous = fixture.handle(Temperature);
        let record = fixture.record(Temperature, "example");

        let result = fixture
            .coordinator
            .shift(message(Temperature, Undo, "missing"))
            .await;

        assert_eq!(
            result.unwrap_err(),
            "Nothing to undo for Temperature (id \"missing\")"
        );
        assert!(!previous.is_finished());
        assert_eq!(fixture.record(Temperature, "example"), record);
        assert_eq!(fixture.coordinator.history.len(), 1);
        assert!(fixture.written.try_recv().is_err());
    })
    .await;
}

#[tokio::test]
#[ignore = "requires a session bus: dbus-run-session -- cargo test -- --include-ignored"]
async fn undo_uses_history_for_the_matching_id() {
    with_coordinator(async |fixture| {
        fixture.shift(Temperature, Relative(500.0), "example").await;
        fixture.shift(Temperature, Relative(500.0), "example").await;
        fixture.shift(Temperature, Absolute(6000.0), "other").await;
        let other = fixture.record(Temperature, "other");

        fixture.shift(Temperature, Undo, "example").await;

        assert_eq!(fixture.record(Temperature, "example").target, 4500.0);
        assert_eq!(fixture.record(Temperature, "other"), other);
    })
    .await;
}

#[tokio::test]
#[ignore = "requires a session bus: dbus-run-session -- cargo test -- --include-ignored"]
async fn undo_uses_history_for_the_matching_property() {
    with_coordinator(async |fixture| {
        fixture.shift(Brightness, Absolute(0.5), "example").await;
        fixture
            .shift(Temperature, Absolute(4500.0), "example")
            .await;
        let temperature = fixture.record(Temperature, "example");

        fixture.shift(Brightness, Undo, "example").await;

        assert_eq!(fixture.record(Brightness, "example").target, 1.0);
        assert_eq!(fixture.record(Temperature, "example"), temperature);
    })
    .await;
}

#[tokio::test]
#[ignore = "requires a session bus: dbus-run-session -- cargo test -- --include-ignored"]
async fn completed_shift_is_not_used_as_an_in_flight_reference() {
    with_coordinator(async |fixture| {
        let mut instant = message(Temperature, Absolute(4500.0), "example");
        instant.end = instant.start;
        fixture.coordinator.shift(instant).await.unwrap();
        assert_eq!(fixture.written.recv().await.unwrap(), (Temperature, 4500.0));
        while !fixture.handle(Temperature).is_finished() {
            tokio::task::yield_now().await;
        }
        fixture.values.lock().unwrap().0 = 4600;

        fixture.shift(Temperature, Linear(1.5), "example").await;

        assert_eq!(
            fixture.record(Temperature, "example"),
            ShiftRecord {
                reference: 4600.0,
                target: 6900.0
            }
        );
    })
    .await;
}

#[tokio::test]
#[ignore = "requires a session bus: dbus-run-session -- cargo test -- --include-ignored"]
async fn failed_property_read_returns_an_error_without_replacing_history() {
    with_coordinator(async |fixture| {
        fixture
            .shift(Temperature, Absolute(4500.0), "example")
            .await;
        let previous = fixture.handle(Temperature);
        let record = fixture.record(Temperature, "example");
        fixture
            .server
            .object_server()
            .remove::<TestRelay, _>("/")
            .await
            .unwrap();

        let result = fixture
            .coordinator
            .shift(message(Temperature, Relative(500.0), "example"))
            .await;

        assert!(
            result
                .unwrap_err()
                .starts_with("Error fetching current Temperature:")
        );
        assert!(previous.is_finished());
        assert_eq!(fixture.record(Temperature, "example"), record);
        assert!(!fixture.coordinator.in_flight.contains_key(&Temperature));
        assert!(fixture.written.try_recv().is_err());
    })
    .await;
}

#[tokio::test]
#[ignore = "requires a session bus: dbus-run-session -- cargo test -- --include-ignored"]
async fn stop_cancels_all_running_properties() {
    with_coordinator(async |fixture| {
        fixture.shift(Brightness, Absolute(0.5), "example").await;
        fixture
            .shift(Temperature, Absolute(4500.0), "example")
            .await;
        let brightness = fixture.handle(Brightness);
        let temperature = fixture.handle(Temperature);

        fixture.coordinator.stop().await;

        assert!(brightness.is_finished());
        assert!(temperature.is_finished());
        assert!(fixture.coordinator.in_flight.is_empty());
    })
    .await;
}
