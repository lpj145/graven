use graven_bench::{capnp_impl as cp, data, fbs_impl as fb, graven_impl as zi, Aligned};

#[test]
fn bank_checksums_match() {
    let (mut z, mut f, mut c) = (zi::Ctx::new(), fb::Ctx::new(), cp::Ctx::new());
    for seed in 0..20 {
        let d = data::bank(seed, (seed % 7) as usize * 3);
        let want = data::checksum_bank(&d);
        assert_eq!(zi::read_all_bank(zi::write_bank(&mut z, &d)), want, "graven streaming seed {seed}");
        assert_eq!(zi::read_all_bank(zi::write_bank_known(&mut z, &d)), want, "graven known seed {seed}");
        assert_eq!(zi::read_all_bank_verified(zi::write_bank(&mut z, &d)), want);
        let v = graven_schemas::bank::Transaction::verified(zi::write_bank(&mut z, &d)).unwrap();
        assert_eq!(zi::read_all_bank_trusted(&v), want);
        assert_eq!(fb::read_all_bank(fb::write_bank(&mut f, &d)), want, "fbs seed {seed}");
        assert_eq!(fb::read_all_bank_verified(fb::write_bank(&mut f, &d)), want);
        let a = Aligned::from(cp::write_bank(&mut c, &d));
        assert_eq!(cp::read_all_bank(a.bytes()), want, "capnp seed {seed}");
        assert_eq!(cp::read_all_bank_verified(a.bytes()), want);
        let one = d.to_shard as u64;
        assert_eq!(zi::read_one_bank(zi::write_bank(&mut z, &d)), one);
        assert_eq!(fb::read_one_bank(fb::write_bank(&mut f, &d)), one);
        assert_eq!(cp::read_one_bank(a.bytes()), one);
    }
}

#[test]
fn frame_checksums_match() {
    let (mut z, mut f, mut c) = (zi::Ctx::new(), fb::Ctx::new(), cp::Ctx::new());
    for seed in 0..20 {
        let d = data::frame(seed, 1 + seed as usize * 5);
        let want = data::checksum_frame(&d);
        assert_eq!(zi::read_all_frame(zi::write_frame(&mut z, &d)), want);
        assert_eq!(zi::read_all_frame_verified(zi::write_frame(&mut z, &d)), want);
        assert_eq!(fb::read_all_frame(fb::write_frame(&mut f, &d)), want);
        assert_eq!(fb::read_all_frame_verified(fb::write_frame(&mut f, &d)), want);
        let a = Aligned::from(cp::write_frame(&mut c, &d));
        assert_eq!(cp::read_all_frame(a.bytes()), want);
        assert_eq!(cp::read_all_frame_verified(a.bytes()), want);
        let one = d.entities[d.entities.len() / 2].hp as u64;
        assert_eq!(zi::read_one_frame(zi::write_frame(&mut z, &d)), one);
        assert_eq!(fb::read_one_frame(fb::write_frame(&mut f, &d)), one);
        assert_eq!(cp::read_one_frame(a.bytes()), one);
    }
}

#[test]
fn telemetry_checksums_match() {
    let (mut z, mut f, mut c) = (zi::Ctx::new(), fb::Ctx::new(), cp::Ctx::new());
    for seed in 0..20 {
        let d = data::telemetry(seed);
        let want = data::checksum_telemetry(&d);
        assert_eq!(zi::read_all_telemetry(zi::write_telemetry(&mut z, &d)), want);
        assert_eq!(zi::read_all_telemetry_verified(zi::write_telemetry(&mut z, &d)), want);
        assert_eq!(fb::read_all_telemetry(fb::write_telemetry(&mut f, &d)), want);
        assert_eq!(fb::read_all_telemetry_verified(fb::write_telemetry(&mut f, &d)), want);
        let a = Aligned::from(cp::write_telemetry(&mut c, &d));
        assert_eq!(cp::read_all_telemetry(a.bytes()), want);
        assert_eq!(cp::read_all_telemetry_verified(a.bytes()), want);
        let one = d.temp.to_bits() as u64;
        assert_eq!(zi::read_one_telemetry(zi::write_telemetry(&mut z, &d)), one);
        assert_eq!(fb::read_one_telemetry(fb::write_telemetry(&mut f, &d)), one);
        assert_eq!(cp::read_one_telemetry(a.bytes()), one);
    }
}

#[test]
fn chunk_checksums_match() {
    let (mut z, mut f, mut c) = (zi::Ctx::new(), fb::Ctx::new(), cp::Ctx::new());
    for seed in 0..20 {
        let d = data::chunk(seed, seed as usize * 100);
        let want = data::checksum_chunk(&d);
        assert_eq!(zi::read_all_chunk(zi::write_chunk(&mut z, &d)), want);
        assert_eq!(zi::read_all_chunk_verified(zi::write_chunk(&mut z, &d)), want);
        assert_eq!(fb::read_all_chunk(fb::write_chunk(&mut f, &d)), want);
        assert_eq!(fb::read_all_chunk_verified(fb::write_chunk(&mut f, &d)), want);
        let a = Aligned::from(cp::write_chunk(&mut c, &d));
        assert_eq!(cp::read_all_chunk(a.bytes()), want);
        assert_eq!(cp::read_all_chunk_verified(a.bytes()), want);
        let one = d.seq as u64;
        assert_eq!(zi::read_one_chunk(zi::write_chunk(&mut z, &d)), one);
        assert_eq!(fb::read_one_chunk(fb::write_chunk(&mut f, &d)), one);
        assert_eq!(cp::read_one_chunk(a.bytes()), one);
    }
}

#[test]
fn batched_and_owned_modes_produce_same_messages() {
    let (mut a, mut o, mut bt) = (zi::Ctx::new(), zi::Ctx::new_owned(), zi::Ctx::new_batched());
    for seed in 0..200 {
        let d = data::bank(seed, (seed % 5) as usize);
        let want = zi::write_bank(&mut a, &d).to_vec();
        assert_eq!(zi::write_bank(&mut o, &d), &want[..]);
        assert_eq!(zi::write_bank(&mut bt, &d), &want[..]);
    }
}
