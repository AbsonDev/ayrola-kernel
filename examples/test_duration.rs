use asupersync::time::wall_now;
use std::time::Duration;
fn main() {
    let now = wall_now();
    let _d = now + Duration::from_secs(2);
}
