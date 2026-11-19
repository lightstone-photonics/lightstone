use lightstone_core::*;
use std::time::Instant;
fn main() {
    let mut sim = Simulator::new(LinkConfig::default());
    let t = Instant::now();
    let r = sim.run();
    println!("cold run {:?}", t.elapsed());
    println!("{:#?}", r.metrics);
    let mut c = sim.config().clone();
    c.fiber.length = 150.0;
    sim.set_config(c.clone());
    let t = Instant::now();
    let _ = sim.run();
    println!("length change {:?}", t.elapsed());
    c.launch.offset = 5e-6;
    sim.set_config(c.clone());
    let t = Instant::now();
    let _ = sim.run();
    println!("offset change {:?}", t.elapsed());
    c.rx.noise_density = 20e-12;
    sim.set_config(c.clone());
    let t = Instant::now();
    let _ = sim.run();
    println!("rx change {:?}", t.elapsed());
}
