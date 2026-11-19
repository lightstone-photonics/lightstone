use lightstone_analysis::*;
use lightstone_core::*;
use std::time::Instant;
fn main() {
    let mut sim = Simulator::new(LinkConfig::default());
    for len in [0.0, 50.0, 100.0, 150.0, 200.0, 300.0] {
        let mut c = sim.config().clone();
        c.fiber.length = len;
        sim.set_config(c);
        let t0 = Instant::now();
        let r = sim.run();
        let t1 = Instant::now();
        let a = analyze(
            &r.rx.signal,
            Some((&r.rx.noiseless, &r.rx.sigma)),
            &r.symbols,
            r.sps,
            &AnalysisOptions::default(),
        );
        let t2 = Instant::now();
        let td = tdecq(
            &r.rx.reference_optical,
            &r.symbols,
            r.sps,
            r.baud,
            &TdecqOptions::default(),
        );
        let t3 = Instant::now();
        let e = eye_density(&r.rx.signal, r.sps, a.delay, a.phase, 600, 400, None);
        let t4 = Instant::now();
        println!("L={len:5}m  BW {:6.1} GHz  TDECQ {:?} dB Ceq {:.2} dB  SER {:.2e} counted {:.2e}  EH {:?} mV EW {:?} UI  rx {:.2} dBm | sim {:?} ana {:?} tdecq {:?} eye {:?} max {}",
          r.metrics.channel_bandwidth.unwrap_or(f64::INFINITY)/1e9, td.tdecq_db.map(|x| (x*100.0).round()/100.0), td.ceq_db, a.ser.unwrap(), a.ser_counted,
          a.eye_height.iter().map(|x| (x*1e4).round()/10.0).collect::<Vec<_>>(), a.eye_width.iter().map(|x| (x*100.0).round()/100.0).collect::<Vec<_>>(), r.metrics.rx_avg_power_dbm,
          t1-t0, t2-t1, t3-t2, t4-t3, e.max_count);
    }
}
