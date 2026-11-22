// Per-op profile of the optimized DPDFNet graph in tract.
use std::collections::HashMap;
use std::time::Instant;
use tract_onnx::prelude::*;
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let path = a.get(1).cloned().unwrap_or("models/dpdfnet8_48khz_hr.onnx".into());
    let proto = tract_onnx::onnx().proto_model_for_path(&path)?;
    let n: usize = proto.metadata_props.iter().find(|e| e.key == "state_size").unwrap().value.parse()?;
    let model = tract_onnx::onnx().model_for_proto_model(&proto)?.into_optimized()?;
    let mut hist: HashMap<String, usize> = HashMap::new();
    for nd in model.nodes() { *hist.entry(nd.op().name().to_string()).or_default() += 1; }
    let mut h: Vec<_> = hist.into_iter().collect(); h.sort_by_key(|x| std::cmp::Reverse(x.1));
    println!("nodes {} ; ops {:?}", model.nodes().len(), &h[..h.len().min(25)]);
    let plan = std::sync::Arc::new(model.into_runnable()?);
    let mut st = plan.spawn()?;
    let mut state = tensor1(&vec![0.0f32; n]);
    let mut tot: HashMap<usize, (f64, String)> = HashMap::new();
    let frames = 300;
    let mut total_wall = 0.0;
    for i in 0..frames + 50 {
        let x = tract_ndarray::Array4::from_shape_fn((1, 1, 481, 2), |(_, _, b, c)| ((b * 7 + c * 3 + i) % 13) as f32 * 0.001).into_tensor();
        let t0 = Instant::now();
        let mut per: Vec<(usize, f64, String)> = vec![];
        let outs = st.run_plan_with_eval(tvec!(x.into_tvalue(), state.clone().into_tvalue()), |ctx, s, node, inp| {
            let t = Instant::now();
            let r = tract_onnx::tract_core::plan::eval(ctx, s, node, inp);
            per.push((node.id, t.elapsed().as_secs_f64() * 1e3, format!("{} [{}]", node.op().name(), node.name)));
            r
        })?;
        let w = t0.elapsed().as_secs_f64() * 1e3;
        state = outs[1].clone().into_tensor();
        if i >= 50 { total_wall += w; for (id, t, nm) in per { let e = tot.entry(id).or_insert((0.0, nm)); e.0 += t; } }
    }
    let mut v: Vec<_> = tot.into_iter().collect(); v.sort_by(|a, b| b.1.0.partial_cmp(&a.1.0).unwrap());
    println!("wall/frame {:.3} ms", total_wall / frames as f64);
    let sum: f64 = v.iter().map(|x| x.1.0).sum::<f64>() / frames as f64;
    println!("sum node time/frame {:.3} ms", sum);
    let mut by_op: HashMap<String, f64> = HashMap::new();
    for (_, (t, nm)) in &v { *by_op.entry(nm.split(' ').next().unwrap().to_string()).or_default() += t / frames as f64; }
    let mut bo: Vec<_> = by_op.into_iter().collect(); bo.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    for (k, t) in bo.iter().take(12) { println!("  {:30} {:.3} ms", k, t); }
    println!("top nodes:");
    for (id, (t, nm)) in v.iter().take(25) { println!("  {:4} {:.3} ms  {}", id, t / frames as f64, &nm[..nm.len().min(110)]); }
    Ok(())
}
fn tensor1(v: &[f32]) -> Tensor { tract_ndarray::Array1::from_vec(v.to_vec()).into_tensor() }
