//  TODO
//
//  - Broken functions:
//
//  mr_for_beacons_global
//  ERROR:  IO error while reading marker: failed to fill whole buffer
//

use lazy_static::lazy_static;
use nng::*;
use nng::options::{Options, RecvTimeout};
use std::time::Duration;
use pgrx::iter::TableIterator;
use pgrx::*;
use serde::de::Deserialize;
use std::env::var;
use std::error::Error;
use core::result::Result;

// pgx specific macros
pg_module_magic!();

lazy_static! {
    static ref SERVICE_URL: String =
        var("RUST_SERVICE_URL").unwrap_or("tcp://127.0.0.1:10234".to_string());
}

const VERSION: Option<&str> = option_env!("CARGO_PKG_VERSION");

const RECV_TIMEOUT_MSEC : u64 = 10000;

fn request_raw(payload : Vec<u8>, timeout_msec : Option<u64>) -> Result<Message, Box<dyn Error + 'static>> {
    let client = Socket::new(Protocol::Req0)?;
    match timeout_msec {
        Some(t) => client.set_opt::<RecvTimeout>(Some(Duration::from_millis(t)))?,
        _       => {}
    }
    client.dial(&SERVICE_URL)?;
    client
        .send(Message::from(payload.as_slice()))
        .map_err(|(_, err)| err)?;
    return Ok(client.recv()?);
}

fn request<T: for<'a> Deserialize<'a>>(
    payload      : Vec<u8>,
    timeout_msec : Option<u64>,
) -> Result<Vec<T>, Box<dyn Error + 'static>> {
    let msg           = request_raw(payload, timeout_msec)?;
    let slice : &[u8] = msg.as_slice();
    rmp_serde::from_slice(slice).or_else(|_| {
        let err: String = rmp_serde::from_slice(slice)?;
        Err(Box::from(format!("Server error: {}", err)))
    })
}

fn contexted_payload(
    context : &str,
    payload : Vec<u8>
) -> Result<Vec<u8>, Box<dyn Error + 'static>> {
    let q : (&str, &str, Vec<u8>) = ("context", context, payload);
    Ok(rmp_serde::to_vec(&q)?)
}

///  Information functions
#[pg_extern]
fn mr_service_url() -> &'static str {
    &SERVICE_URL
}

#[pg_extern]
fn mr_connector() ->  &'static str { &VERSION.unwrap_or("unknown") }

fn mr_service_wrapped() -> Result<String, Box<dyn Error + 'static>> {
    let payload  = rmp_serde::to_vec(&"ver")?;
    let response = request_raw(payload, Some(RECV_TIMEOUT_MSEC))?;
    let s        = rmp_serde::from_slice(response.as_slice())?;
    return Ok(s);
}

#[pg_extern]
fn mr_service() -> String {
    match mr_service_wrapped() {
        Err(e) => format!("{}", e),
        Ok(s)  => s
    }
}

/// Basic functions

#[pg_extern]
fn mr_node_score_superposition(
    ego: &str,
    target: &str,
) -> Result<
    TableIterator<'static, (name!(ego, String), name!(target, String), name!(score, f64))>,
    Box<dyn Error + 'static>,
> {
    let payload  = rmp_serde::to_vec(&((("src", "=", ego), ("dest", "=", target)), ()))?;
    let response = request(payload, Some(RECV_TIMEOUT_MSEC))?;
    return Ok(TableIterator::new(response));
}

#[pg_extern]
fn mr_node_score(
    ego: &str,
    target: &str,
    context: &str,
) -> Result<
    TableIterator<'static, (name!(ego, String), name!(target, String), name!(score, f64))>,
    Box<dyn Error + 'static>,
> {
    let payload  = rmp_serde::to_vec(&((("src", "=", ego), ("dest", "=", target)), ()))?;
    let payload  = if context.is_empty() { payload } else { contexted_payload(context, payload)? };
    let response = request(payload, Some(RECV_TIMEOUT_MSEC))?;
    return Ok(TableIterator::new(response));
}

#[pg_extern]
fn mr_node_score_linear_sum(
    ego: &str,
    target: &str,
) -> Result<
    TableIterator<'static, (name!(ego, String), name!(target, String), name!(score, f64))>,
    Box<dyn Error + 'static>,
> {
    let payload  = rmp_serde::to_vec(&((("src", "=", ego), ("dest", "=", target)), (), "null"))?;
    let response = request(payload, Some(RECV_TIMEOUT_MSEC))?;
    return Ok(TableIterator::new(response));
}


fn mr_scores0(
    ego: &str,
    hide_personal: bool,
    start_with: Option<String>,
    score_lt: Option<f64>,
    score_lte: Option<f64>,
    score_gt: Option<f64>,
    score_gte: Option<f64>,
    limit: Option<i32>
) -> Result<
    Vec<u8>,
    Box<dyn Error + 'static>,
> {
    let (lcmp, lt) = match (score_lt, score_lte) {
        (Some(lt), None) => ("<", lt),
        (None, Some(lte)) => ("<=", lte),
        (None, None) => ("<", f64::MIN),
        _ => return Err(Box::from("either lt or lte allowed!"))
    };
    let (gcmp, gt) = match (score_gt, score_gte) {
        (Some(gt), None) => (">", gt),
        (None, Some(gte)) => (">=", gte),
        (None, None) => (">", f64::MAX),
        _ => return Err(Box::from("either gt or gte allowed!"))
    };
    let binding = start_with.unwrap_or(String::new());
    let q = ((
              ("src", "=", ego),
              ("target", "like", binding.as_str()),
              ("hide_personal", hide_personal),
              ("score", gcmp, gt),
              ("score", lcmp, lt),
              ("limit", limit)
             ),
             ());
    rmp_serde::to_vec(&q)
        .map_err(|e| e.into())
}

#[pg_extern]
fn mr_scores_superposition(
    ego: &str,
    start_with: Option<String>,
    score_lt: Option<f64>,
    score_lte: Option<f64>,
    score_gt: Option<f64>,
    score_gte: Option<f64>,
    limit: Option<i32>
) -> Result<
    TableIterator<'static, (name!(ego, String), name!(target, String), name!(score, f64))>,
    Box<dyn Error + 'static>,
> {
    let payload = mr_scores0(
        ego,
        false,
        start_with,
        score_lt, score_lte,
        score_gt, score_gte,
        limit
    )?;
    let response = request(payload, Some(RECV_TIMEOUT_MSEC))?;
    return Ok(TableIterator::new(response));
}

#[pg_extern]
fn mr_scores(
    ego: &str,
    hide_personal: bool,
    context: &str,
    start_with: Option<String>,
    score_lt: Option<f64>,
    score_lte: Option<f64>,
    score_gt: Option<f64>,
    score_gte: Option<f64>,
    limit: Option<i32>
) -> Result<
    TableIterator<'static, (name!(ego, String), name!(target, String), name!(score, f64))>,
    Box<dyn Error + 'static>,
> {
    let payload = mr_scores0(
        ego,
        hide_personal,
        start_with,
        score_lt, score_lte,
        score_gt, score_gte,
        limit
    )?;
    let payload  = if context.is_empty() { payload } else { contexted_payload(context, payload)? };
    let response = request(payload, Some(RECV_TIMEOUT_MSEC))?;
    return Ok(TableIterator::new(response));
}

#[pg_extern]
fn mr_scores_linear_sum(
    src: &str,
) -> Result<
    TableIterator<'static, (name!(ego, String), name!(target, String), name!(score, f64))>,
    Box<dyn Error + 'static>,
> {
    let payload  = rmp_serde::to_vec(&((("src", "=", src), ), (), "null"))?;
    let response = request(payload, Some(RECV_TIMEOUT_MSEC))?;
    return Ok(TableIterator::new(response));
}

#[pg_extern]
fn mr_score_linear_sum(
    src: &str,
    dest: &str
) -> Result<
    TableIterator<'static, (name!(ego, String), name!(target, String), name!(score, f64))>,
    Box<dyn Error + 'static>,
> {
    let payload = rmp_serde::to_vec(&((("src", "=", src), ("dest", "=", dest)), (), "null"))?;
    let response = request(payload, Some(RECV_TIMEOUT_MSEC))?;
    return Ok(TableIterator::new(response));
}

/// Modify functions

#[pg_extern]
fn mr_put_edge(
    src: &str,
    dest: &str,
    weight: f64,
    context: &str,
) -> Result<
    TableIterator<'static, (name!(ego, String), name!(target, String), name!(score, f64))>,
    Box<dyn Error>,
> {
    let payload  = rmp_serde::to_vec(&(((src, dest, weight), ), ()))?;
    let payload  = if context.is_empty() { payload } else { contexted_payload(context, payload)? };
    let response = request(payload, Some(RECV_TIMEOUT_MSEC))?;
    return Ok(TableIterator::new(response));
}

#[pg_extern]
fn mr_delete_edge(
    ego: &str,
    target: &str,
    context: &str,
) -> Result<&'static str, Box<dyn Error + 'static>> {
    let payload     = rmp_serde::to_vec(&((("src", "delete", ego), ("dest", "delete", target)), ()))?;
    let payload     = if context.is_empty() { payload } else { contexted_payload(context, payload)? };
    let _ : Vec<()> = request(payload, Some(RECV_TIMEOUT_MSEC))?;
    return Ok("Ok");
}

#[pg_extern]
fn mr_delete_node(
    ego: &str,
    context: &str,
) -> Result<&'static str, Box<dyn Error + 'static>> {
    let payload     = rmp_serde::to_vec(&((("src", "delete", ego), ), ()))?;
    let payload     = if context.is_empty() { payload } else { contexted_payload(context, payload)? };
    let _ : Vec<()> = request(payload, Some(RECV_TIMEOUT_MSEC))?;
    return Ok("Ok");
}

/// Gravity functions

#[pg_extern]
fn mr_graph(
    ego: &str,
    focus: &str,
    context: &str,
    positive_only: bool,
    limit: Option<i32>
) -> Result<
    TableIterator<'static, (name!(ego, String), name!(target, String), name!(score, f64))>,
    Box<dyn Error + 'static>,
> {
    let payload  = rmp_serde::to_vec(&(((ego, "gravity", focus), positive_only, limit), ()))?;
    let payload  = if context.is_empty() { payload } else { contexted_payload(context, payload)? };
    let response = request(payload, Some(RECV_TIMEOUT_MSEC))?;
    return Ok(TableIterator::new(response));
}

#[pg_extern]
fn mr_nodes(
    ego: &str,
    focus: &str,
    context: &str,
    positive_only: bool,
    limit: Option<i32>
) -> Result<
    TableIterator<'static, (name!(node, String), name!(weight, f64))>,
    Box<dyn Error + 'static>,
> {
    let payload  = rmp_serde::to_vec(&(((ego, "gravity_nodes", focus), positive_only, limit), ()))?;
    let payload  = if context.is_empty() { payload } else { contexted_payload(context, payload)? };
    let response = request(payload, Some(RECV_TIMEOUT_MSEC))?;
    return Ok(TableIterator::new(response));
}

#[pg_extern]
fn mr_for_beacons_global() -> Result<
    TableIterator<'static, (name!(ego, String), name!(target, String), name!(score, f64))>,
    Box<dyn Error + 'static>,
> {
    //    NOTE
    //    Slow function, no timeout.
    let payload  = rmp_serde::to_vec(&("for_beacons_global", ()))?;
    let response = request(payload, None)?;
    return Ok(TableIterator::new(response));
}

/// list functions

#[pg_extern]
fn mr_nodelist(context: &str) -> Result<
    TableIterator<'static, (name!(id, String), )>,
    Box<dyn Error + 'static>,
> {
    let payload  = rmp_serde::to_vec(&("nodes", ()))?;
    let payload  = if context.is_empty() { payload } else { contexted_payload(context, payload)? };
    let response = request(payload, Some(RECV_TIMEOUT_MSEC))?;
    return Ok(TableIterator::new(response));
}

#[pg_extern]
fn mr_edgelist(
    context: &str
) -> Result<
    TableIterator<'static, (name!(source, String), name!(target, String), name!(weight, f64))>,
    Box<dyn Error + 'static>,
> {
    let payload  = rmp_serde::to_vec(&("edges", ()))?;
    let payload  = if context.is_empty() { payload } else { contexted_payload(context, payload)? };
    let response = request(payload, Some(RECV_TIMEOUT_MSEC))?;
    return Ok(TableIterator::new(response));
}

// connected nodes

#[pg_extern]
fn mr_connected(
    ego: &str,
    context: &str,
) -> Result<
    TableIterator<'static, (name!(source, String), name!(target, String))>,
    Box<dyn Error + 'static>,
> {
    let payload  = rmp_serde::to_vec(&(((ego, "connected"), ), ()))?;
    let payload  = if context.is_empty() { payload } else { contexted_payload(context, payload)? };
    let response = request(payload, Some(RECV_TIMEOUT_MSEC))?;
    return Ok(TableIterator::new(response));
}

#[pg_extern]
fn mr_zerorec() -> Result<
    String,
    Box<dyn Error + 'static>,
> {
    let payload  = rmp_serde::to_vec(&(("zerorec"), ()))?;
    let response = request_raw(payload, None)?;
    let s        = rmp_serde::from_slice(response.as_slice())?;
    return Ok(s);
}

//
//  Testing
//

#[cfg(any(test, feature = "pg_test"))]
#[pg_schema]
mod tests {
    use pgrx::prelude::*;

    fn put_testing_edges() {
        let _ = crate::mr_put_edge("U0cd6bd2dde4f","B7f628ad203b5",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U7c9ce0ac22b7","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U9a2c85753a6d","C070e739180d6",9.0,"").unwrap();
        let _ = crate::mr_put_edge("U1c285703fc63","Bad1c69de7837",7.0,"").unwrap();
        let _ = crate::mr_put_edge("U25982b736535","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U8a78048d60f7","B92e4a185c654",3.0,"").unwrap();
        let _ = crate::mr_put_edge("U663cd1f1e343","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U9a89e0679dec","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U09cf1f359454","B3c467fb437b2",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("U5b09928b977a","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U585dfead09c6","C6d52e861b366",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("U02fbd7c8df4c","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uc1158424318a","C78d6fac93d00",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U7a8d8324441d","Cbbf2df46955b",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U4f530cfe771e","B9c01ce5718d1",0.0,"").unwrap();
        let _ = crate::mr_put_edge("U8a78048d60f7","Cd6c9d5cba220",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Cf4b448ef8618","U499f24158a40",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U4d6816b2416e","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U6942e4590e93","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U389f9f24b31c","Cbbf2df46955b",4.0,"").unwrap();
        let _ = crate::mr_put_edge("Ub01f4ad1b03f","B73a44e2bbd44",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uab16119974a0","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U09cf1f359454","B5a1c1d3d0140",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("U8a78048d60f7","U6d2f25cc4264",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U1df3e39ebe59","Bea16f01b8cc5",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C599f6e6f6b64","U26aca0e369c7",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Udb60bbb285ca","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uf5a84bada7fb","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U79466f73dc0c","B7f628ad203b5",6.0,"").unwrap();
        let _ = crate::mr_put_edge("U6d2f25cc4264","B3c467fb437b2",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uf59dcd0bc354","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ucb84c094edba","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ud7002ae5a86c","B75a44a52fa29",-2.0,"").unwrap();
        let _ = crate::mr_put_edge("Uc3c31b8a022f","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U80e22da6d8c4","C6acd550a4ef3",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uf2b0a6b1d423","B5eb4c6be535a",5.0,"").unwrap();
        let _ = crate::mr_put_edge("B9c01ce5718d1","U499f24158a40",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ub01f4ad1b03f","U499f24158a40",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U867a75db12ae","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U99a0f1f7e6ee","Bd90a1cf73384",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U6629a0a8ef04","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ub7f9dfb6a7a5","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uf31403bd4e20","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U0e214fef4f03","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U0e6659929c53","Cffd169930956",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Cd1c25e32ad21","Ucd424ac24c15",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uac897fe92894","B9c01ce5718d1",-2.0,"").unwrap();
        let _ = crate::mr_put_edge("Bc4addf09b79f","U0cd6bd2dde4f",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U638f5c19326f","B9cade9992fb9",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U290a1ab9d54a","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U3c63a9b6115a","Bad1c69de7837",2.0,"").unwrap();
        let _ = crate::mr_put_edge("U016217c34c6e","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U389f9f24b31c","C6acd550a4ef3",6.0,"").unwrap();
        let _ = crate::mr_put_edge("U9e42f6dab85a","Cbbf2df46955b",3.0,"").unwrap();
        let _ = crate::mr_put_edge("U99a0f1f7e6ee","C4d1d582c53c3",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Be2b46c17f1da","U80e22da6d8c4",1.0,"").unwrap();
        let _ = crate::mr_put_edge("B5e7178dd70bb","Ucbd309d6fcc0",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U7a8d8324441d","U1c285703fc63",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("C4893c40e481d","Udece0afd9a8b",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U9e42f6dab85a","B3c467fb437b2",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U8842ed397bb7","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ue70d59cc8e3f","B9c01ce5718d1",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U5c827d7de115","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ue94281e36fe8","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U8a78048d60f7","Bdf39d0e1daf5",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("U18a178de1dfb","B70df5dbab8c3",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U4f530cfe771e","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uad577360d968","B5eb4c6be535a",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U526f361717a8","Cee9901f0f22c",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uccc3c7395af6","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C2bbd63b00224","U80e22da6d8c4",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Cb3c476a45037","Ue40b938f47a4",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U1c634fdd7c82","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C22e1102411ce","U6661263fb410",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U57b6f30fc663","Bed5126bc655d",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("U6661263fb410","Cf92f90725ffc",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uef7fbf45ef11","C2bbd63b00224",8.0,"").unwrap();
        let _ = crate::mr_put_edge("U09cf1f359454","Ba5d64165e5d5",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ub01f4ad1b03f","U79466f73dc0c",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U40096feaa029","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U09cf1f359454","B5eb4c6be535a",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("U8a78048d60f7","B499bfc56e77b",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("U3c63a9b6115a","Cf92f90725ffc",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U9ce5721e93cf","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ud04c89aaf453","B4f14b223b56d",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ue7a29d5409f2","Udece0afd9a8b",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U38fdca6685ca","Cf77494dc63d7",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U83282a51b600","Be2b46c17f1da",0.0,"").unwrap();
        let _ = crate::mr_put_edge("U83e829a2e822","B7f628ad203b5",14.0,"").unwrap();
        let _ = crate::mr_put_edge("Bc896788cd2ef","U1bcba4fd7175",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uf2b0a6b1d423","C67e4476fda28",6.0,"").unwrap();
        let _ = crate::mr_put_edge("C9028c7415403","Udece0afd9a8b",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U01814d1ec9ff","U499f24158a40",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uadeb43da4abb","B0e230e9108dd",4.0,"").unwrap();
        let _ = crate::mr_put_edge("U1bcba4fd7175","C264c56d501db",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U8a78048d60f7","B73a44e2bbd44",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ud982a6dee46f","Be7145faf15cb",1.0,"").unwrap();
        let _ = crate::mr_put_edge("B0a87a669fc28","U34252014c05b",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U0e6659929c53","Cb967536095de",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ucb37b247402a","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C0f834110f700","U38fdca6685ca",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U72f88cf28226","Cb11edc3d0bc7",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U499f24158a40","C0166be581dd4",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U5d9b4e4a7baf","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U526f361717a8","C52d41a9ad558",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ue7a29d5409f2","Cb76829a425d9",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U499f24158a40","Cf4b448ef8618",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U48dcd166b0bd","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uadeb43da4abb","C30e7409c2d5f",2.0,"").unwrap();
        let _ = crate::mr_put_edge("U05e4396e2382","B7f628ad203b5",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uf3b5141d73f3","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U8a78048d60f7","Cb11edc3d0bc7",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U18a178de1dfb","B1533941e2773",1.0,"").unwrap();
        let _ = crate::mr_put_edge("B506fff6cfc22","Ub7f9dfb6a7a5",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uad577360d968","C2bbd63b00224",9.0,"").unwrap();
        let _ = crate::mr_put_edge("Uda5b03b660d7","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U7a8d8324441d","C4f2dafca724f",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ucb9952d31a9e","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U8a78048d60f7","Bd7a8bfcf3337",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C1ccb4354d684","Ue202d5b01f8d",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ud5b22ebf52f2","Cd6c9d5cba220",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U8a78048d60f7","Ba5d64165e5d5",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("U0da9e22a248b","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uab20c65d180d","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uf5096f6ab14e","C6aebafa4fe8e",8.0,"").unwrap();
        let _ = crate::mr_put_edge("Uef7fbf45ef11","C588ffef22463",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ccae34b3da05e","Ub93799d9400e",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U8a78048d60f7","B9c01ce5718d1",3.0,"").unwrap();
        let _ = crate::mr_put_edge("Uc35c445325f5","B75a44a52fa29",2.0,"").unwrap();
        let _ = crate::mr_put_edge("U362d375c067c","Ce06bda6030fe",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uaa4e2be7a87a","Cfdde53c79a2d",3.0,"").unwrap();
        let _ = crate::mr_put_edge("U09cf1f359454","B75a44a52fa29",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Bb5f87c1621d5","Ub01f4ad1b03f",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U016217c34c6e","B3c467fb437b2",2.0,"").unwrap();
        let _ = crate::mr_put_edge("U9a2c85753a6d","Udece0afd9a8b",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U6d2f25cc4264","B63fbe1427d09",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ub01f4ad1b03f","C5782d559baad",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C3b855f713d19","U704bd6ecde75",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U016217c34c6e","Cb76829a425d9",2.0,"").unwrap();
        let _ = crate::mr_put_edge("Ub0205d5d96d0","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U499f24158a40","Ba3c4a280657d",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U0c17798eaab4","Udece0afd9a8b",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ud7002ae5a86c","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U35eb26fc07b4","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uc1158424318a","Bdf39d0e1daf5",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U96a8bbfce56f","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C588ffef22463","Uef7fbf45ef11",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U72f88cf28226","B3f6f837bc345",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ba3c4a280657d","U499f24158a40",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ua9d9d5da3948","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U8a78048d60f7","Bd90a1cf73384",3.0,"").unwrap();
        let _ = crate::mr_put_edge("U638f5c19326f","B9c01ce5718d1",2.0,"").unwrap();
        let _ = crate::mr_put_edge("U80e22da6d8c4","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Udf6d8127c2c6","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U362d375c067c","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U83282a51b600","C9462ca240ceb",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U638f5c19326f","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U3116d27854ab","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U499f24158a40","C54972a5fbc16",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ub93799d9400e","B9c01ce5718d1",5.0,"").unwrap();
        let _ = crate::mr_put_edge("U9e42f6dab85a","C15d8dfaceb75",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U1bcba4fd7175","Be2b46c17f1da",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("B8a531802473b","U016217c34c6e",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U01814d1ec9ff","Bb78026d99388",-11.0,"").unwrap();
        let _ = crate::mr_put_edge("Ue7a29d5409f2","C4893c40e481d",4.0,"").unwrap();
        let _ = crate::mr_put_edge("Cb11edc3d0bc7","U8a78048d60f7",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ub01f4ad1b03f","Bb1e3630d2f4a",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U0cd6bd2dde4f","B92e4a185c654",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U09cf1f359454","B45d72e29f004",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("Cab47a458295f","U6d2f25cc4264",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U7dd2b82154e0","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U1f348902b446","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uad577360d968","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U861750348e9f","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ue55b928fa8dd","Bed5126bc655d",3.0,"").unwrap();
        let _ = crate::mr_put_edge("U016217c34c6e","U9a89e0679dec",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uc35c445325f5","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U09cf1f359454","U8a78048d60f7",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C6aebafa4fe8e","U9a2c85753a6d",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ucdffb8ab5145","Cf8fb8c05c116",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U0cd6bd2dde4f","B9c01ce5718d1",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U59abf06369c3","Cda989f4b466d",1.0,"").unwrap();
        let _ = crate::mr_put_edge("B4f00e7813add","U09cf1f359454",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U8a78048d60f7","B75a44a52fa29",3.0,"").unwrap();
        let _ = crate::mr_put_edge("U80e22da6d8c4","U0c17798eaab4",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ub01f4ad1b03f","U09cf1f359454",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U21769235b28d","C801f204d0da8",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uddd01c7863e9","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U9a2c85753a6d","B3c467fb437b2",9.0,"").unwrap();
        let _ = crate::mr_put_edge("U06a4bdf76bf7","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U43dcf522b4dd","B3b3f2ecde430",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("C264c56d501db","U1bcba4fd7175",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ua4041a93bdf4","B9c01ce5718d1",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uc3c31b8a022f","B45d72e29f004",3.0,"").unwrap();
        let _ = crate::mr_put_edge("Uaaf5341090c6","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uf2b0a6b1d423","C399b6349ab02",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U45578f837ab8","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ub01f4ad1b03f","U6d2f25cc4264",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uf5096f6ab14e","B3b3f2ecde430",3.0,"").unwrap();
        let _ = crate::mr_put_edge("Ub01f4ad1b03f","U8a78048d60f7",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ub01f4ad1b03f","B5eb4c6be535a",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uc1158424318a","Cfdde53c79a2d",6.0,"").unwrap();
        let _ = crate::mr_put_edge("Ub192fb5e4fee","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U7eaa146a4793","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Udece0afd9a8b","Uadeb43da4abb",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("U6d2f25cc4264","Bdf39d0e1daf5",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("U96a7841bc98d","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U7ac570b5840f","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U80e22da6d8c4","Cbbf2df46955b",5.0,"").unwrap();
        let _ = crate::mr_put_edge("U7399d6656581","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ud7186ef65120","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U9a2c85753a6d","C78ad459d3b81",4.0,"").unwrap();
        let _ = crate::mr_put_edge("Ub01f4ad1b03f","B5a1c1d3d0140",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("U526c52711601","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U8a78048d60f7","B25c85fe0df2d",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uc1158424318a","C6acd550a4ef3",1.0,"").unwrap();
        let _ = crate::mr_put_edge("B310b66ab31fb","U6d2f25cc4264",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U499f24158a40","C4b2b6fd8fa9a",1.0,"").unwrap();
        let _ = crate::mr_put_edge("B70df5dbab8c3","U09cf1f359454",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U1bcba4fd7175","U09cf1f359454",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U18a178de1dfb","B75a44a52fa29",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uadeb43da4abb","C9462ca240ceb",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("U9a89e0679dec","Bb78026d99388",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U89a6e30efb07","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U8a78048d60f7","B491d307dfe01",3.0,"").unwrap();
        let _ = crate::mr_put_edge("C7c4d9ca4623e","U8aa2e2623fa5",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ub901d5e0edca","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U01814d1ec9ff","C1c86825bd597",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Udece0afd9a8b","C357396896bd0",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ub01f4ad1b03f","B4f00e7813add",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U1c285703fc63","U9e42f6dab85a",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ua50dd76e5a75","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U1e41b5f3adff","B310b66ab31fb",5.0,"").unwrap();
        let _ = crate::mr_put_edge("Cc2b3069cbe5d","Ub01f4ad1b03f",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uaa4e2be7a87a","Uadeb43da4abb",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ucd424ac24c15","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U431131a166be","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U59abf06369c3","B7f628ad203b5",3.0,"").unwrap();
        let _ = crate::mr_put_edge("U146915ad287e","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U1bcba4fd7175","B45d72e29f004",-9.0,"").unwrap();
        let _ = crate::mr_put_edge("Ud5f1a29622d1","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U05e4396e2382","Bad1c69de7837",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("Cd795a41fe71d","U362d375c067c",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U389f9f24b31c","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U72f88cf28226","B310b66ab31fb",1.0,"").unwrap();
        let _ = crate::mr_put_edge("B4f14b223b56d","Ud04c89aaf453",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uf6ce05bc4e5a","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U1e41b5f3adff","U6d2f25cc4264",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U83e829a2e822","B0e230e9108dd",-4.0,"").unwrap();
        let _ = crate::mr_put_edge("Ucbd309d6fcc0","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U8f0839032839","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uf2b0a6b1d423","C6a2263dc469e",3.0,"").unwrap();
        let _ = crate::mr_put_edge("Ueb1e69384e4e","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C89c123f7bcf5","U8842ed397bb7",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ufb826ea158e5","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U26aca0e369c7","C4893c40e481d",2.0,"").unwrap();
        let _ = crate::mr_put_edge("Ue7a29d5409f2","Uaa4e2be7a87a",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uf5096f6ab14e","C4893c40e481d",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("U18a178de1dfb","B3f6f837bc345",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U6d2f25cc4264","C25639690ee57",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U6d2f25cc4264","Ud9df8116deba",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ca8ceac412e6f","U4ba2e4e81c0e",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Be29b4af3f7a5","Uc35c445325f5",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U1188b2dfb294","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U01814d1ec9ff","U02fbd7c8df4c",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Cb07d467c1c5e","U8a78048d60f7",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U8aa2e2623fa5","B9c01ce5718d1",-2.0,"").unwrap();
        let _ = crate::mr_put_edge("Ub01f4ad1b03f","B3b3f2ecde430",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("U88e719e6257d","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U8a78048d60f7","Cf4b448ef8618",2.0,"").unwrap();
        let _ = crate::mr_put_edge("U4c619411e5de","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C9a2135edf7ff","U83282a51b600",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ub01f4ad1b03f","B19ea554faf29",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ba5d64165e5d5","U1e41b5f3adff",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uf5096f6ab14e","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ucfe743b8deb1","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U9605bd4d1218","B75a44a52fa29",4.0,"").unwrap();
        let _ = crate::mr_put_edge("B499bfc56e77b","Uc1158424318a",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U1c285703fc63","Cd59e6cd7e104",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U83e829a2e822","Be2b46c17f1da",-8.0,"").unwrap();
        let _ = crate::mr_put_edge("U8a78048d60f7","B45d72e29f004",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("U499f24158a40","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Cb117f464e558","U26aca0e369c7",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U4ba2e4e81c0e","B7f628ad203b5",-2.0,"").unwrap();
        let _ = crate::mr_put_edge("U18a178de1dfb","B19ea554faf29",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Cfd59a206c07d","U99a0f1f7e6ee",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C8ece5c618ac1","U21769235b28d",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uadeb43da4abb","Cc9f863ff681b",2.0,"").unwrap();
        let _ = crate::mr_put_edge("Ubd3c556b8a25","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U389f9f24b31c","Cdcddfb230cb5",5.0,"").unwrap();
        let _ = crate::mr_put_edge("Uc1158424318a","Cc9f863ff681b",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U26aca0e369c7","C6acd550a4ef3",4.0,"").unwrap();
        let _ = crate::mr_put_edge("C8c753f46c014","U8842ed397bb7",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C78d6fac93d00","Uc1158424318a",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U9a2c85753a6d","C357396896bd0",8.0,"").unwrap();
        let _ = crate::mr_put_edge("U389f9f24b31c","Cd59e6cd7e104",3.0,"").unwrap();
        let _ = crate::mr_put_edge("U8c33fbcc06d7","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Bf3a0a1165271","U9a89e0679dec",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U09cf1f359454","B70df5dbab8c3",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Cb967536095de","U0e6659929c53",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C0b19d314485e","Uaa4e2be7a87a",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U09cf1f359454","Bc896788cd2ef",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uc35c445325f5","B9c01ce5718d1",4.0,"").unwrap();
        let _ = crate::mr_put_edge("U01814d1ec9ff","B9c01ce5718d1",10.0,"").unwrap();
        let _ = crate::mr_put_edge("C25639690ee57","U6d2f25cc4264",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ue202d5b01f8d","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U362d375c067c","Bad1c69de7837",0.0,"").unwrap();
        let _ = crate::mr_put_edge("U1c285703fc63","C67e4476fda28",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ue7a29d5409f2","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U8a78048d60f7","B60d725feca77",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("U8a78048d60f7","Bfefe4e25c870",3.0,"").unwrap();
        let _ = crate::mr_put_edge("Uc4f728b0d87f","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U9a2c85753a6d","Cdcddfb230cb5",4.0,"").unwrap();
        let _ = crate::mr_put_edge("Uf5096f6ab14e","Cb14487d862b3",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U682c3380036f","C7986cd8a648a",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U02fbd7c8df4c","Bd7a8bfcf3337",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U7a8d8324441d","Cbbf2df46955b",5.0,"").unwrap();
        let _ = crate::mr_put_edge("U8a78048d60f7","U6240251593cd",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U499f24158a40","C8d80016b8292",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uc35c445325f5","B8a531802473b",-5.0,"").unwrap();
        let _ = crate::mr_put_edge("U704bd6ecde75","B9c01ce5718d1",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("U77f496546efa","B9c01ce5718d1",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("U6d2f25cc4264","B7f628ad203b5",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ub01f4ad1b03f","B10d3f548efc4",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U7a8d8324441d","Cd06fea6a395f",9.0,"").unwrap();
        let _ = crate::mr_put_edge("U682c3380036f","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U36055bb45e5c","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U7cdd7999301e","B7f628ad203b5",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U526f361717a8","Cf40e8fb326bc",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U946ae258c4b5","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("B944097cdd968","Ue40b938f47a4",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U2f08dff8dbdb","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ub01f4ad1b03f","B3f6f837bc345",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U6661263fb410","Cc01e00342d63",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U80e22da6d8c4","Cb76829a425d9",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ccb7dc40f1513","U6661263fb410",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U83282a51b600","C9a2135edf7ff",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Cb76829a425d9","Ue7a29d5409f2",1.0,"").unwrap();
        let _ = crate::mr_put_edge("B45d72e29f004","U26aca0e369c7",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ue6cc7bfa0efd","B5e7178dd70bb",-7.0,"").unwrap();
        let _ = crate::mr_put_edge("Uac897fe92894","Be2b46c17f1da",2.0,"").unwrap();
        let _ = crate::mr_put_edge("B73a44e2bbd44","U8a78048d60f7",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ue7a29d5409f2","C399b6349ab02",5.0,"").unwrap();
        let _ = crate::mr_put_edge("Cfa08a39f9bb9","Ubebfe0c8fc29",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Cdcddfb230cb5","Udece0afd9a8b",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ub01f4ad1b03f","Bb5f87c1621d5",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U7a8d8324441d","C78d6fac93d00",2.0,"").unwrap();
        let _ = crate::mr_put_edge("U18a178de1dfb","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U2cd96f1b2ea6","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ub01f4ad1b03f","B3c467fb437b2",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("C0cd490b5fb6a","Uad577360d968",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U80e22da6d8c4","Be2b46c17f1da",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U0f63ee3db59b","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U09cf1f359454","B499bfc56e77b",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("U7a975ca7e0b0","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C2cb023b6bcef","Ucb84c094edba",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U016217c34c6e","C4e0db8dec53e",4.0,"").unwrap();
        let _ = crate::mr_put_edge("Cc931cd2de143","Ud7002ae5a86c",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U0c17798eaab4","C4893c40e481d",7.0,"").unwrap();
        let _ = crate::mr_put_edge("U1c285703fc63","U016217c34c6e",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U4a82930ca419","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U682c3380036f","U6240251593cd",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U2a62e985bcd5","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U1e6a314ef612","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uab766aeb8fd2","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U18a178de1dfb","B4f00e7813add",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uc4ebbce44401","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ud7002ae5a86c","Cc931cd2de143",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ue5c10787d0db","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U499f24158a40","B79efabc4d8bf",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U044c5bf57a97","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U99deecf5a281","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U9605bd4d1218","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U3de789cac826","B9c01ce5718d1",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U7a8d8324441d","C888c86d096d0",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U5f148383594f","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U499f24158a40","C10872dc9b863",1.0,"").unwrap();
        let _ = crate::mr_put_edge("B0e230e9108dd","U9a89e0679dec",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ufec0de2f341d","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U8a78048d60f7","C2e31b4b1658f",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U09cf1f359454","B25c85fe0df2d",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uc3db248a6e7f","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U09cf1f359454","C81f3f954b643",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U6eab54d64086","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C0a576fc389d9","U1bcba4fd7175",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U4a6d6f193ae0","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U6d2f25cc4264","B3f6f837bc345",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U01814d1ec9ff","C6d52e861b366",3.0,"").unwrap();
        let _ = crate::mr_put_edge("U362d375c067c","Cd795a41fe71d",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uc676bd7563ec","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U6d2f25cc4264","B3b3f2ecde430",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("U585dfead09c6","B9c01ce5718d1",2.0,"").unwrap();
        let _ = crate::mr_put_edge("Cfd47f43ac9cf","U704bd6ecde75",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U72f88cf28226","Cd6c9d5cba220",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Cdd49e516723a","U704bd6ecde75",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U26aca0e369c7","Be2b46c17f1da",7.0,"").unwrap();
        let _ = crate::mr_put_edge("U6249d53929c4","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uad577360d968","C588ffef22463",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("U8a78048d60f7","B3c467fb437b2",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("Bf34ee3bfc12b","U6240251593cd",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uf2b0a6b1d423","Bb78026d99388",9.0,"").unwrap();
        let _ = crate::mr_put_edge("Ue202d5b01f8d","B9c01ce5718d1",2.0,"").unwrap();
        let _ = crate::mr_put_edge("U6d2f25cc4264","B310b66ab31fb",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U35eb26fc07b4","C90290100a953",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Cc9f863ff681b","Uc1158424318a",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uf5ee43a1b729","C9218f86f6286",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C888c86d096d0","U7a8d8324441d",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U499f24158a40","Bfefe4e25c870",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U499f24158a40","C6f84810d3cd9",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Cd6c9d5cba220","Ud5b22ebf52f2",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U99a0f1f7e6ee","C96bdee4f11e2",-18.0,"").unwrap();
        let _ = crate::mr_put_edge("U4a82930ca419","C2d9ab331aed7",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C4818c4ed20bf","U499f24158a40",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U585dfead09c6","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ucd424ac24c15","Cd1c25e32ad21",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U389f9f24b31c","Bad1c69de7837",2.0,"").unwrap();
        let _ = crate::mr_put_edge("U20d01ad4d96b","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ue7a29d5409f2","Cfdde53c79a2d",5.0,"").unwrap();
        let _ = crate::mr_put_edge("U5ef3d593e46e","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U7382ac807a4f","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U88137a4bf483","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U5c827d7de115","B69723edfec8a",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U1bcba4fd7175","Cd4417a5d718e",5.0,"").unwrap();
        let _ = crate::mr_put_edge("Ue202d5b01f8d","C1ccb4354d684",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U6d2f25cc4264","B9c01ce5718d1",4.0,"").unwrap();
        let _ = crate::mr_put_edge("Udece0afd9a8b","Cdcddfb230cb5",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ufca294ffe3a5","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C81f3f954b643","U09cf1f359454",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U02fbd7c8df4c","B75a44a52fa29",7.0,"").unwrap();
        let _ = crate::mr_put_edge("U049bf307d470","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U1c285703fc63","C30e7409c2d5f",4.0,"").unwrap();
        let _ = crate::mr_put_edge("U8a78048d60f7","Be5bb2f3d56cb",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("U4d82230c274a","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("B10d3f548efc4","U99a0f1f7e6ee",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uc3c31b8a022f","B3c467fb437b2",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("C90290100a953","U35eb26fc07b4",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uadeb43da4abb","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U18a178de1dfb","B310b66ab31fb",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U35eb26fc07b4","Be2b46c17f1da",0.0,"").unwrap();
        let _ = crate::mr_put_edge("Ccbd85b8513f3","U499f24158a40",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U5ee57577b2bd","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U1bcba4fd7175","Bc896788cd2ef",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U0cd6bd2dde4f","C7062e90f7422",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U6d2f25cc4264","Be2b46c17f1da",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("C4d1d582c53c3","U99a0f1f7e6ee",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U11722d2113bf","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U59abf06369c3","Cb117f464e558",-3.0,"").unwrap();
        let _ = crate::mr_put_edge("B491d307dfe01","U499f24158a40",1.0,"").unwrap();
        let _ = crate::mr_put_edge("B25c85fe0df2d","Uef7fbf45ef11",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Bdf39d0e1daf5","Uc1158424318a",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U9a2c85753a6d","C3e84102071d1",6.0,"").unwrap();
        let _ = crate::mr_put_edge("U2371cf61799b","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U8a78048d60f7","B63fbe1427d09",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("U8a78048d60f7","Cd5983133fb67",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Cc616eded7a99","U0f63ee3db59b",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U34252014c05b","B19ea554faf29",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U6622a635b181","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U0f63ee3db59b","B9c01ce5718d1",-4.0,"").unwrap();
        let _ = crate::mr_put_edge("Uf6ce05bc4e5a","U499f24158a40",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U8a78048d60f7","Cbce32a9b256a",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U00ace0c36154","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U9a89e0679dec","Bf3a0a1165271",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U01814d1ec9ff","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ub1a7f706910f","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U01814d1ec9ff","B63fbe1427d09",-3.0,"").unwrap();
        let _ = crate::mr_put_edge("U03eaee0e3052","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U0cd6bd2dde4f","Bc4addf09b79f",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U1bcba4fd7175","B4f00e7813add",3.0,"").unwrap();
        let _ = crate::mr_put_edge("Ub01f4ad1b03f","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U996b5f6b8bec","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U9e42f6dab85a","Bad1c69de7837",3.0,"").unwrap();
        let _ = crate::mr_put_edge("U26aca0e369c7","C599f6e6f6b64",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U09cf1f359454","Bdf39d0e1daf5",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uf8bf10852d43","B253177f84f08",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U7a8d8324441d","B7f628ad203b5",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U43dcf522b4dd","B9c01ce5718d1",2.0,"").unwrap();
        let _ = crate::mr_put_edge("C13e2a35d917a","Uf6ce05bc4e5a",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ub01f4ad1b03f","B8a531802473b",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uf5ee43a1b729","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U499f24158a40","C247501543b60",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C2e31b4b1658f","U8a78048d60f7",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U3c63a9b6115a","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C94bb73c10a06","Uef7fbf45ef11",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C357396896bd0","Udece0afd9a8b",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C6acd550a4ef3","Uc1158424318a",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U016217c34c6e","C3e84102071d1",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U18a178de1dfb","Bc4addf09b79f",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U499f24158a40","Cfe90cbd73eab",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U80e22da6d8c4","C30e7409c2d5f",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uc8bb404462a4","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U09cf1f359454","Bf3a0a1165271",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("U14a3c81256ab","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uadeb43da4abb","C2bbd63b00224",7.0,"").unwrap();
        let _ = crate::mr_put_edge("Ub01f4ad1b03f","Be2b46c17f1da",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("U3f840973f9b5","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Caa62fc21e191","U4ba2e4e81c0e",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U02fbd7c8df4c","Bad1c69de7837",-5.0,"").unwrap();
        let _ = crate::mr_put_edge("U1bcba4fd7175","B73a44e2bbd44",3.0,"").unwrap();
        let _ = crate::mr_put_edge("U37f5b0f1e914","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U80e22da6d8c4","U9e42f6dab85a",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Cb95e21215efa","U499f24158a40",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U35108003593e","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("B1533941e2773","U79466f73dc0c",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U1e41b5f3adff","Ba5d64165e5d5",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U118afa836f11","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U682c3380036f","Bf34ee3bfc12b",4.0,"").unwrap();
        let _ = crate::mr_put_edge("Udece0afd9a8b","Uc3c31b8a022f",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("U6d2f25cc4264","U1c285703fc63",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U7a8d8324441d","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U0d47e4861ef0","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U1779c42930af","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uc67c60f504ce","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U36ddff1a63d8","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U6661263fb410","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U9ce5721e93cf","B68247950d9c0",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uf8bf10852d43","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U8456b2b56820","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U389f9f24b31c","Uc3c31b8a022f",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U9a89e0679dec","Cd06fea6a395f",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("U9e42f6dab85a","C6a2263dc469e",5.0,"").unwrap();
        let _ = crate::mr_put_edge("Ub01f4ad1b03f","B0a87a669fc28",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Cf40e8fb326bc","U526f361717a8",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U4ba2e4e81c0e","Cb117f464e558",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U47b466d57da1","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U526f361717a8","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ub93799d9400e","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C524134905072","Ucb84c094edba",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Cd59e6cd7e104","U80e22da6d8c4",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uaa4e2be7a87a","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uc3a2aab8a776","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U9a89e0679dec","Cbbf2df46955b",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("U38fdca6685ca","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Cbe89905f07d3","Ub01f4ad1b03f",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Bed5126bc655d","Uc4ebbce44401",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U9605bd4d1218","B8a531802473b",2.0,"").unwrap();
        let _ = crate::mr_put_edge("Ueb139752b907","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ub93799d9400e","B73a44e2bbd44",5.0,"").unwrap();
        let _ = crate::mr_put_edge("Cee9901f0f22c","U526f361717a8",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ub01f4ad1b03f","Cc2b3069cbe5d",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U7a8d8324441d","B3b3f2ecde430",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ubebfe0c8fc29","Cfa08a39f9bb9",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C6587e913fbbe","U6661263fb410",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U895fd30e1e2a","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U0cd6bd2dde4f","C5782d559baad",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U6d2f25cc4264","B25c85fe0df2d",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("U1bcba4fd7175","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U016217c34c6e","B8a531802473b",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ccc25a77bfa2a","U77f496546efa",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U6240251593cd","Bf34ee3bfc12b",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U8a78048d60f7","C357396896bd0",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uf2b0a6b1d423","Cb76829a425d9",8.0,"").unwrap();
        let _ = crate::mr_put_edge("Ue7a29d5409f2","U016217c34c6e",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ucdffb8ab5145","B9c01ce5718d1",2.0,"").unwrap();
        let _ = crate::mr_put_edge("U01814d1ec9ff","B75a44a52fa29",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Cac6ca02355da","U6d2f25cc4264",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ub01f4ad1b03f","Bc4addf09b79f",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U831a82104a9e","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U0c17798eaab4","U389f9f24b31c",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U0453a921d0e7","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U8a78048d60f7","Ud9df8116deba",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ucd424ac24c15","B9c01ce5718d1",2.0,"").unwrap();
        let _ = crate::mr_put_edge("U1bcba4fd7175","B9c01ce5718d1",9.0,"").unwrap();
        let _ = crate::mr_put_edge("Ua7759a06a90a","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U3c63a9b6115a","B75a44a52fa29",5.0,"").unwrap();
        let _ = crate::mr_put_edge("Bea16f01b8cc5","U1df3e39ebe59",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U1eafbaaf9536","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uef7fbf45ef11","C3fd1fdebe0e9",9.0,"").unwrap();
        let _ = crate::mr_put_edge("U73057a8e8ebf","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Cffd169930956","U0e6659929c53",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U01814d1ec9ff","B3b3f2ecde430",-3.0,"").unwrap();
        let _ = crate::mr_put_edge("Uf2b0a6b1d423","C4e0db8dec53e",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ua9f1d3f8ee78","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U83282a51b600","B9c01ce5718d1",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uf5096f6ab14e","U9e42f6dab85a",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("U6d2f25cc4264","Bfefe4e25c870",4.0,"").unwrap();
        let _ = crate::mr_put_edge("U80e22da6d8c4","C070e739180d6",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C8343a6a576ff","U02fbd7c8df4c",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Udece0afd9a8b","C599f6e6f6b64",2.0,"").unwrap();
        let _ = crate::mr_put_edge("U77f496546efa","C9462ca240ceb",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("Cc42c3eeb9d20","U8a78048d60f7",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uf2b0a6b1d423","Ce1a7d8996eb0",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("U1bcba4fd7175","B70df5dbab8c3",2.0,"").unwrap();
        let _ = crate::mr_put_edge("Ub10b78df4f63","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uaa4e2be7a87a","C35678a54ef5f",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U59abf06369c3","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U3de789cac826","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U72f88cf28226","C7722465c957a",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U9605bd4d1218","C801f204d0da8",3.0,"").unwrap();
        let _ = crate::mr_put_edge("U8a78048d60f7","B10d3f548efc4",3.0,"").unwrap();
        let _ = crate::mr_put_edge("U02be55e5fdb2","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U9a89e0679dec","U7a8d8324441d",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Be7145faf15cb","Ud982a6dee46f",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U7f5fca21e1e5","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Cd06fea6a395f","Uaa4e2be7a87a",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U7a8d8324441d","C78ad459d3b81",6.0,"").unwrap();
        let _ = crate::mr_put_edge("Udf0362755172","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U28f934dc948e","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Bb1e3630d2f4a","U34252014c05b",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U6661263fb410","B75a44a52fa29",3.0,"").unwrap();
        let _ = crate::mr_put_edge("U495c3bb411e1","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uadeb43da4abb","Bd49e3dac97b0",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ub93799d9400e","Cd4417a5d718e",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C399b6349ab02","Uf2b0a6b1d423",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ue73fabd3d39a","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U4dac6797a9cc","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ua29a81d30ef9","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ud5b22ebf52f2","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("B79efabc4d8bf","U499f24158a40",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U83282a51b600","C16dfdd8077c8",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U1c285703fc63","U9a2c85753a6d",1.0,"").unwrap();
        let _ = crate::mr_put_edge("B19ea554faf29","U34252014c05b",1.0,"").unwrap();
        let _ = crate::mr_put_edge("B75a44a52fa29","U01814d1ec9ff",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C35678a54ef5f","Uaa4e2be7a87a",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uc3c31b8a022f","Bb78026d99388",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U09cf1f359454","Be5bb2f3d56cb",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("U4dd243415525","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Cb62aea64ea97","U0e6659929c53",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uef7fbf45ef11","B25c85fe0df2d",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uefe16d246c36","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U09cf1f359454","B8a531802473b",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("C5127d08eb786","Ucd424ac24c15",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U7a8d8324441d","Be2b46c17f1da",5.0,"").unwrap();
        let _ = crate::mr_put_edge("U8a78048d60f7","U1c285703fc63",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uf2b0a6b1d423","C6a2263dc469e",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uef7fbf45ef11","B0e230e9108dd",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("U53eb1f0bdcd2","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U8a78048d60f7","Uad577360d968",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U1afee48387d4","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U09cf1f359454","B4f14b223b56d",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("B3c467fb437b2","U9e42f6dab85a",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U9a2c85753a6d","C30fef1977b4a",8.0,"").unwrap();
        let _ = crate::mr_put_edge("U8a78048d60f7","B19ea554faf29",3.0,"").unwrap();
        let _ = crate::mr_put_edge("U6240251593cd","B9c01ce5718d1",-4.0,"").unwrap();
        let _ = crate::mr_put_edge("U99a0f1f7e6ee","C1f41b842849c",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uac897fe92894","Cb117f464e558",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U704bd6ecde75","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U09cf1f359454","U0cd6bd2dde4f",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ucb84c094edba","C524134905072",1.0,"").unwrap();
        let _ = crate::mr_put_edge("B19d70698e3d8","Uf8bf10852d43",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Cda989f4b466d","U59abf06369c3",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ub01f4ad1b03f","B1533941e2773",3.0,"").unwrap();
        let _ = crate::mr_put_edge("U83e829a2e822","B5eb4c6be535a",3.0,"").unwrap();
        let _ = crate::mr_put_edge("U34252014c05b","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ud9df8116deba","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U01814d1ec9ff","Bd7a8bfcf3337",3.0,"").unwrap();
        let _ = crate::mr_put_edge("U918f8950c4e5","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Cfdde53c79a2d","Uef7fbf45ef11",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ue6cc7bfa0efd","B30bf91bf5845",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U09cf1f359454","B3b3f2ecde430",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("C63e21d051dda","U638f5c19326f",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uf2b0a6b1d423","C30e7409c2d5f",9.0,"").unwrap();
        let _ = crate::mr_put_edge("Ue7a29d5409f2","C9028c7415403",3.0,"").unwrap();
        let _ = crate::mr_put_edge("U09cf1f359454","Bd49e3dac97b0",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ua34e02cf30a6","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C279db553a831","U99a0f1f7e6ee",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ub01f4ad1b03f","B45d72e29f004",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("U01814d1ec9ff","B491d307dfe01",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("U99a0f1f7e6ee","Cfd59a206c07d",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C52d41a9ad558","U526f361717a8",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U7462db3b65c4","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U79466f73dc0c","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ue7a29d5409f2","Uc3c31b8a022f",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uf9ecad50b7e1","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U1bcba4fd7175","C0a576fc389d9",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uef7fbf45ef11","C94bb73c10a06",3.0,"").unwrap();
        let _ = crate::mr_put_edge("U3614888a1bdc","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U8a78048d60f7","Ud5b22ebf52f2",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uf8bf10852d43","B4115d364e05b",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U57b6f30fc663","B30bf91bf5845",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U72f88cf28226","U6d2f25cc4264",0.0,"").unwrap();
        let _ = crate::mr_put_edge("U3c63a9b6115a","Be5bb2f3d56cb",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C7722465c957a","U72f88cf28226",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ub7f9dfb6a7a5","B506fff6cfc22",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Udece0afd9a8b","C9028c7415403",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U79466f73dc0c","B45d72e29f004",5.0,"").unwrap();
        let _ = crate::mr_put_edge("U67bf00435429","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U3bbfefd5319e","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U99a0f1f7e6ee","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U09cf1f359454","B3f6f837bc345",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U8a78048d60f7","B4f14b223b56d",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("U0e6659929c53","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U6661263fb410","C31dac67e313b",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C55a114ca6e7c","U0e6659929c53",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ue328d7da3b59","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C4b2b6fd8fa9a","U499f24158a40",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U9e42f6dab85a","C0b19d314485e",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("U6d2f25cc4264","Ba3c4a280657d",2.0,"").unwrap();
        let _ = crate::mr_put_edge("Ub01f4ad1b03f","B70df5dbab8c3",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uf8eb8562f949","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U7a8d8324441d","C30fef1977b4a",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uc35c445325f5","Be29b4af3f7a5",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uebe87839ab3e","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ud826f91f9025","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U7a8d8324441d","B5eb4c6be535a",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uf2b0a6b1d423","Cdcddfb230cb5",3.0,"").unwrap();
        let _ = crate::mr_put_edge("U9605bd4d1218","Bd7a8bfcf3337",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U4e7d43caba8f","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U499f24158a40","B9c01ce5718d1",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U7a8d8324441d","B3b3f2ecde430",9.0,"").unwrap();
        let _ = crate::mr_put_edge("U83282a51b600","B45d72e29f004",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("U4db49066d45a","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ub01f4ad1b03f","B60d725feca77",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("U21769235b28d","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U80e22da6d8c4","Cd59e6cd7e104",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U26aca0e369c7","C9028c7415403",8.0,"").unwrap();
        let _ = crate::mr_put_edge("U161742354fef","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Udece0afd9a8b","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("B9cade9992fb9","U638f5c19326f",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U3b6ea55b4098","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U05e4396e2382","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U09cf1f359454","B0e230e9108dd",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("U499f24158a40","U6d2f25cc4264",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U79466f73dc0c","Be2b46c17f1da",4.0,"").unwrap();
        let _ = crate::mr_put_edge("U27847df66cb4","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C9218f86f6286","Uf5ee43a1b729",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U6d2f25cc4264","Bb78026d99388",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("U9a2c85753a6d","B3b3f2ecde430",6.0,"").unwrap();
        let _ = crate::mr_put_edge("U8a78048d60f7","Bd49e3dac97b0",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uf5096f6ab14e","C9462ca240ceb",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U1bcba4fd7175","B0e230e9108dd",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("U9a2c85753a6d","Uf5096f6ab14e",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U0a5d1c56f5a1","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ue7a29d5409f2","Uf2b0a6b1d423",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ub01f4ad1b03f","Bd90a1cf73384",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ucb84c094edba","C2cb023b6bcef",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Udfbfcd087e6b","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U3bf4a5894df1","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ubebfe0c8fc29","Bfefe4e25c870",3.0,"").unwrap();
        let _ = crate::mr_put_edge("U9e42f6dab85a","C070e739180d6",2.0,"").unwrap();
        let _ = crate::mr_put_edge("U6d2f25cc4264","C6f84810d3cd9",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U14a3c81256ab","B9c01ce5718d1",0.0,"").unwrap();
        let _ = crate::mr_put_edge("Cd5983133fb67","U8a78048d60f7",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U675d1026fe95","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U0cd6bd2dde4f","B75a44a52fa29",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ue7a29d5409f2","Ce1a7d8996eb0",5.0,"").unwrap();
        let _ = crate::mr_put_edge("Ue40b938f47a4","B9c01ce5718d1",0.0,"").unwrap();
        let _ = crate::mr_put_edge("U0e6659929c53","Cb62aea64ea97",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U732b06e17fc6","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C1c86825bd597","U01814d1ec9ff",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U09cf1f359454","Bb78026d99388",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("U393de9ce9ec4","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U389f9f24b31c","Cbce32a9b256a",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U499f24158a40","C96bdee4f11e2",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uc76658319bfe","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U389f9f24b31c","C4893c40e481d",3.0,"").unwrap();
        let _ = crate::mr_put_edge("Uef7fbf45ef11","B7f628ad203b5",7.0,"").unwrap();
        let _ = crate::mr_put_edge("Uad577360d968","Bad1c69de7837",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U65bb6831c537","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U3b78f50182c7","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ud982a6dee46f","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U499f24158a40","Cdeab5b39cc2a",1.0,"").unwrap();
        let _ = crate::mr_put_edge("B7f628ad203b5","U7a8d8324441d",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Udece0afd9a8b","C4893c40e481d",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C96bdee4f11e2","U499f24158a40",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U0b4010c6af8e","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ub01f4ad1b03f","B0e230e9108dd",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("U02fbd7c8df4c","C8343a6a576ff",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C30fef1977b4a","U7a8d8324441d",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U77f496546efa","Ccc25a77bfa2a",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uf6ce05bc4e5a","C13e2a35d917a",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ucbb6d026b66f","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U6d2f25cc4264","Cfe90cbd73eab",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U0c17798eaab4","B3c467fb437b2",2.0,"").unwrap();
        let _ = crate::mr_put_edge("Ue6cc7bfa0efd","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ue4f003e63773","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U9e42f6dab85a","U80e22da6d8c4",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U7cdd7999301e","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uef7fbf45ef11","Cfdde53c79a2d",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U7a8d8324441d","C94bb73c10a06",9.0,"").unwrap();
        let _ = crate::mr_put_edge("U8a78048d60f7","Bb78026d99388",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("U6d2f25cc4264","B499bfc56e77b",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("U0c17798eaab4","Ce1a7d8996eb0",6.0,"").unwrap();
        let _ = crate::mr_put_edge("C7062e90f7422","U01814d1ec9ff",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Cf8fb8c05c116","Ucdffb8ab5145",1.0,"").unwrap();
        let _ = crate::mr_put_edge("B60d725feca77","U80e22da6d8c4",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C070e739180d6","U80e22da6d8c4",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C3e84102071d1","U016217c34c6e",1.0,"").unwrap();
        let _ = crate::mr_put_edge("B69723edfec8a","U5c827d7de115",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U0c17798eaab4","B0e230e9108dd",3.0,"").unwrap();
        let _ = crate::mr_put_edge("U0c17798eaab4","C4e0db8dec53e",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uf2b0a6b1d423","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U389f9f24b31c","Cfc639b9aa3e0",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uad577360d968","C0cd490b5fb6a",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ucc8ea98c2b41","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uc3c31b8a022f","U1c285703fc63",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U3c63a9b6115a","B9c01ce5718d1",3.0,"").unwrap();
        let _ = crate::mr_put_edge("B3b3f2ecde430","U7a8d8324441d",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U38fdca6685ca","B9c01ce5718d1",0.0,"").unwrap();
        let _ = crate::mr_put_edge("Ua1ca6a97ea28","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U9a2c85753a6d","C78ad459d3b81",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uc1158424318a","C67e4476fda28",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ub01f4ad1b03f","U01814d1ec9ff",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ua5a9eab9732d","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U4ba2e4e81c0e","Ca8ceac412e6f",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Cfe90cbd73eab","U499f24158a40",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U79466f73dc0c","B1533941e2773",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C8d80016b8292","U499f24158a40",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uf6ce05bc4e5a","Bf843e315d71b",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U6661263fb410","C6587e913fbbe",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U8a78048d60f7","Bb1e3630d2f4a",3.0,"").unwrap();
        let _ = crate::mr_put_edge("Ucd424ac24c15","C5127d08eb786",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uc1158424318a","C4e0db8dec53e",4.0,"").unwrap();
        let _ = crate::mr_put_edge("U6d2f25cc4264","Bad1c69de7837",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ud7002ae5a86c","C7a807e462b65",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C3c17b70c3357","U3de789cac826",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U5cfee124371b","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Bd90a1cf73384","U99a0f1f7e6ee",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uc3a349f521e1","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U83282a51b600","B7f628ad203b5",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U0cd6bd2dde4f","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U26aca0e369c7","Cb117f464e558",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ce1a7d8996eb0","Uf5096f6ab14e",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U09cf1f359454","Bad1c69de7837",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ub01f4ad1b03f","B25c85fe0df2d",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("U9a89e0679dec","C6aebafa4fe8e",8.0,"").unwrap();
        let _ = crate::mr_put_edge("U01814d1ec9ff","B8a531802473b",8.0,"").unwrap();
        let _ = crate::mr_put_edge("Ub93799d9400e","B75a44a52fa29",5.0,"").unwrap();
        let _ = crate::mr_put_edge("Ub01f4ad1b03f","Bf34ee3bfc12b",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U34252014c05b","B0a87a669fc28",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ud18285ef1202","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ub01f4ad1b03f","Cbe89905f07d3",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U7c88b933c58d","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U4ba2e4e81c0e","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U8889e390d38b","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U9e972ae23870","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U389f9f24b31c","U7a8d8324441d",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uf5096f6ab14e","Ce1a7d8996eb0",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U09cf1f359454","B4f00e7813add",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Bd7a8bfcf3337","U02fbd7c8df4c",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C2d9ab331aed7","U4a82930ca419",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C247501543b60","U499f24158a40",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U9a2c85753a6d","Cfdde53c79a2d",4.0,"").unwrap();
        let _ = crate::mr_put_edge("U80e22da6d8c4","C613f00c1333c",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C31dac67e313b","U6661263fb410",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U864ef33f7249","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uf91b831f1eb7","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ua85bc934db95","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C67e4476fda28","U1c285703fc63",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U09cf1f359454","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U80e22da6d8c4","C3e84102071d1",4.0,"").unwrap();
        let _ = crate::mr_put_edge("U9605bd4d1218","Cab47a458295f",3.0,"").unwrap();
        let _ = crate::mr_put_edge("U6d2f25cc4264","C992d8370db6b",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Cbce32a9b256a","U389f9f24b31c",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uc1158424318a","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U09cf1f359454","B63fbe1427d09",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("B63fbe1427d09","U1c285703fc63",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uf5096f6ab14e","B60d725feca77",8.0,"").unwrap();
        let _ = crate::mr_put_edge("U1bcba4fd7175","Bfefe4e25c870",5.0,"").unwrap();
        let _ = crate::mr_put_edge("U09cf1f359454","B9c01ce5718d1",2.0,"").unwrap();
        let _ = crate::mr_put_edge("U83282a51b600","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uac897fe92894","B7f628ad203b5",1.0,"").unwrap();
        let _ = crate::mr_put_edge("B8120aa1edccb","Ue40b938f47a4",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ubbe66e390603","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ueb139752b907","U79466f73dc0c",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U1c285703fc63","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U0e6659929c53","B9c01ce5718d1",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U09cf1f359454","B491d307dfe01",2.0,"").unwrap();
        let _ = crate::mr_put_edge("U18a178de1dfb","B73a44e2bbd44",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U38fdca6685ca","C958e7588ae1c",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U72f88cf28226","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ue70d59cc8e3f","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U35eb26fc07b4","B60d725feca77",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Cdeab5b39cc2a","U499f24158a40",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U0f63ee3db59b","Cbcf72c7e6061",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C4e0db8dec53e","U0c17798eaab4",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U8a78048d60f7","B7f628ad203b5",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uc78a29f47b21","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C472b59eeafa5","U4a82930ca419",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U6d2f25cc4264","C247501543b60",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Cfc639b9aa3e0","U389f9f24b31c",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U99deecf5a281","B9c01ce5718d1",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Bfefe4e25c870","U499f24158a40",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ua12e78308f49","B75a44a52fa29",4.0,"").unwrap();
        let _ = crate::mr_put_edge("U1d5b8c2a3400","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ucdffb8ab5145","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uaa4e2be7a87a","Cd06fea6a395f",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U682c3380036f","B75a44a52fa29",2.0,"").unwrap();
        let _ = crate::mr_put_edge("Ue202d5b01f8d","C637133747308",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U6d2f25cc4264","Cab47a458295f",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U0c17798eaab4","Cd06fea6a395f",8.0,"").unwrap();
        let _ = crate::mr_put_edge("Ub01f4ad1b03f","Bd7a8bfcf3337",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U8a78048d60f7","U01814d1ec9ff",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ua01529fb0d57","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U611323f9392c","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uf5ee43a1b729","B47cc49866c37",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uac897fe92894","C9462ca240ceb",0.0,"").unwrap();
        let _ = crate::mr_put_edge("U21769235b28d","C481cd737c873",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C6f84810d3cd9","U499f24158a40",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ub93c197b25c5","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uc3c31b8a022f","C78d6fac93d00",3.0,"").unwrap();
        let _ = crate::mr_put_edge("Udece0afd9a8b","C4f2dafca724f",8.0,"").unwrap();
        let _ = crate::mr_put_edge("U9361426a2e51","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uaa4e2be7a87a","B0e230e9108dd",2.0,"").unwrap();
        let _ = crate::mr_put_edge("Ub01f4ad1b03f","Bb78026d99388",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("U8a78048d60f7","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U8a78048d60f7","Cdcddfb230cb5",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ub01f4ad1b03f","U0cd6bd2dde4f",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ud5b22ebf52f2","B310b66ab31fb",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U802de6b3675a","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U8a78048d60f7","B0e230e9108dd",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("U09cf1f359454","U6d2f25cc4264",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uad577360d968","Cbce32a9b256a",3.0,"").unwrap();
        let _ = crate::mr_put_edge("U09cf1f359454","Be29b4af3f7a5",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("U5e1dd853cab5","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C958e7588ae1c","U38fdca6685ca",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ub152bb6d4a86","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U47b466d57da1","Bad1c69de7837",-3.0,"").unwrap();
        let _ = crate::mr_put_edge("U016217c34c6e","Ca0a6aea6c82e",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U18a178de1dfb","B491d307dfe01",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U79466f73dc0c","B9c01ce5718d1",-6.0,"").unwrap();
        let _ = crate::mr_put_edge("Ucfb9f0586d9e","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U704bd6ecde75","C3b855f713d19",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uc1158424318a","C0b19d314485e",4.0,"").unwrap();
        let _ = crate::mr_put_edge("U1c285703fc63","Uad577360d968",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ce49159fe9d01","U6661263fb410",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U6d2f25cc4264","B8a531802473b",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("U8a78048d60f7","B310b66ab31fb",4.0,"").unwrap();
        let _ = crate::mr_put_edge("Ub22f9ca70b59","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U499f24158a40","B491d307dfe01",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ue40b938f47a4","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ud04c89aaf453","U8a78048d60f7",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U9605bd4d1218","B5a1c1d3d0140",2.0,"").unwrap();
        let _ = crate::mr_put_edge("U2cb58c48703b","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ud04c89aaf453","B73a44e2bbd44",4.0,"").unwrap();
        let _ = crate::mr_put_edge("Ub01f4ad1b03f","B9c01ce5718d1",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uc4ebbce44401","Bed5126bc655d",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U8a78048d60f7","B79efabc4d8bf",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Be5bb2f3d56cb","U3c63a9b6115a",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U8842ed397bb7","C89c123f7bcf5",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uceaf0448e060","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uc1158424318a","B7f628ad203b5",8.0,"").unwrap();
        let _ = crate::mr_put_edge("U0f63ee3db59b","Cc616eded7a99",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U26aca0e369c7","B45d72e29f004",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ubeded808a9c0","B9c01ce5718d1",6.0,"").unwrap();
        let _ = crate::mr_put_edge("B30bf91bf5845","Ue6cc7bfa0efd",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U005d51b8771c","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ubcf610883f95","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U499f24158a40","Cb95e21215efa",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U606a687682ec","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C16dfdd8077c8","U83282a51b600",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C1f41b842849c","U99a0f1f7e6ee",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ue20d37fe1d62","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U1e5391821528","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U1c285703fc63","U6d2f25cc4264",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U09cf1f359454","B60d725feca77",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("B3f6f837bc345","U6d2f25cc4264",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U704bd6ecde75","Cfd47f43ac9cf",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ua12e78308f49","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U4a82930ca419","C472b59eeafa5",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U8a78048d60f7","Ub93799d9400e",1.0,"").unwrap();
        let _ = crate::mr_put_edge("B47cc49866c37","Uf5ee43a1b729",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U6d2f25cc4264","B491d307dfe01",3.0,"").unwrap();
        let _ = crate::mr_put_edge("U7a8d8324441d","C3fd1fdebe0e9",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U0c17798eaab4","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U0ff6902d8945","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U0e6659929c53","C55a114ca6e7c",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U7a8d8324441d","U6d2f25cc4264",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U638f5c19326f","C63e21d051dda",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U8ec514590d15","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U6240251593cd","B75a44a52fa29",4.0,"").unwrap();
        let _ = crate::mr_put_edge("C7986cd8a648a","U682c3380036f",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C637133747308","Ue202d5b01f8d",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U9605bd4d1218","B9c01ce5718d1",2.0,"").unwrap();
        let _ = crate::mr_put_edge("B68247950d9c0","U9ce5721e93cf",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Bf843e315d71b","Uf6ce05bc4e5a",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U5f8c0e9c8cc4","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U01814d1ec9ff","B5a1c1d3d0140",5.0,"").unwrap();
        let _ = crate::mr_put_edge("U8a78048d60f7","B5eb4c6be535a",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ubebfe0c8fc29","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U6d2f25cc4264","B79efabc4d8bf",2.0,"").unwrap();
        let _ = crate::mr_put_edge("U9c1051c9bb99","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("B4115d364e05b","Uf8bf10852d43",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ue40b938f47a4","B8120aa1edccb",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U4ff50cbb890f","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U660f0dfe3117","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U26aca0e369c7","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uef7fbf45ef11","U6d2f25cc4264",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uc1158424318a","B499bfc56e77b",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U499f24158a40","Cd172fb3fdc41",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uaa4e2be7a87a","C0b19d314485e",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U6d2f25cc4264","B5eb4c6be535a",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("U17789c126682","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ubd48a3c8df1e","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Bad1c69de7837","Uad577360d968",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U8a78048d60f7","Bf34ee3bfc12b",3.0,"").unwrap();
        let _ = crate::mr_put_edge("U80e22da6d8c4","C2bbd63b00224",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U35eb26fc07b4","Cb117f464e558",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("U57a6591c7ee1","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U8aa2e2623fa5","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U8a78048d60f7","Cc42c3eeb9d20",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C78ad459d3b81","U9a2c85753a6d",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uf2b0a6b1d423","C3fd1fdebe0e9",7.0,"").unwrap();
        let _ = crate::mr_put_edge("Uaa4e2be7a87a","B7f628ad203b5",9.0,"").unwrap();
        let _ = crate::mr_put_edge("U8b70c7c00136","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uf75d4cbe5430","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U8a78048d60f7","Be2b46c17f1da",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("U84f274f30e33","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U26451935eec8","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U83e829a2e822","Bad1c69de7837",-4.0,"").unwrap();
        let _ = crate::mr_put_edge("U80e22da6d8c4","C35678a54ef5f",5.0,"").unwrap();
        let _ = crate::mr_put_edge("U5d0cd6daa146","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uf5096f6ab14e","U7a8d8324441d",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C9462ca240ceb","Uf5096f6ab14e",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U9a2c85753a6d","C4893c40e481d",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("U18a178de1dfb","Bf34ee3bfc12b",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U3de789cac826","C3c17b70c3357",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U6661263fb410","Ce49159fe9d01",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U09cf1f359454","B310b66ab31fb",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ua4041a93bdf4","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U389f9f24b31c","C6aebafa4fe8e",6.0,"").unwrap();
        let _ = crate::mr_put_edge("U21769235b28d","C6d52e861b366",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uad577360d968","C6a2263dc469e",5.0,"").unwrap();
        let _ = crate::mr_put_edge("Udece0afd9a8b","Bad1c69de7837",9.0,"").unwrap();
        let _ = crate::mr_put_edge("U1e41b5f3adff","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U9a2c85753a6d","C6aebafa4fe8e",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ueb139752b907","B1533941e2773",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Udece0afd9a8b","U1c285703fc63",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U09cf1f359454","Be2b46c17f1da",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ub01f4ad1b03f","B92e4a185c654",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Cbcf72c7e6061","U0f63ee3db59b",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Cf92f90725ffc","U6661263fb410",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U6d2f25cc4264","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U6106ae1092fa","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U037b51a34f3c","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U526f361717a8","B9c01ce5718d1",0.0,"").unwrap();
        let _ = crate::mr_put_edge("Ud9df8116deba","B310b66ab31fb",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U6661263fb410","C22e1102411ce",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Udc7c82928598","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U22ad914a7065","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U21769235b28d","C8ece5c618ac1",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U99a0f1f7e6ee","B10d3f548efc4",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ce06bda6030fe","U362d375c067c",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U8a78048d60f7","Bf3a0a1165271",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uc44834086c03","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ue55b928fa8dd","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U7a54f2f24cf6","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C5167c9b3d347","U362d375c067c",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U6727ddef0614","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ubeded808a9c0","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C613f00c1333c","U80e22da6d8c4",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U8a78048d60f7","B0a87a669fc28",3.0,"").unwrap();
        let _ = crate::mr_put_edge("U6240251593cd","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uef7fbf45ef11","C588ffef22463",4.0,"").unwrap();
        let _ = crate::mr_put_edge("Uc1158424318a","C9028c7415403",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ue40b938f47a4","B944097cdd968",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ub01f4ad1b03f","B310b66ab31fb",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U016217c34c6e","U80e22da6d8c4",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U362d375c067c","C5060d0101429",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U9a2c85753a6d","Ce1a7d8996eb0",2.0,"").unwrap();
        let _ = crate::mr_put_edge("U43dcf522b4dd","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uad577360d968","C399b6349ab02",6.0,"").unwrap();
        let _ = crate::mr_put_edge("U9a2c85753a6d","C6a2263dc469e",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ud2c791d9e879","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C992d8370db6b","U6d2f25cc4264",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uf6ce05bc4e5a","B9c01ce5718d1",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U016217c34c6e","C15d8dfaceb75",8.0,"").unwrap();
        let _ = crate::mr_put_edge("Ub93799d9400e","B491d307dfe01",2.0,"").unwrap();
        let _ = crate::mr_put_edge("Ubd9c1e76bb53","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U6d2f25cc4264","Cac6ca02355da",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U389f9f24b31c","C4f2dafca724f",5.0,"").unwrap();
        let _ = crate::mr_put_edge("Ua6dfa92ad74d","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U0c17798eaab4","Uad577360d968",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U0667457dabfe","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U8842ed397bb7","C8c753f46c014",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U09cf1f359454","B73a44e2bbd44",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U6d2f25cc4264","C8d80016b8292",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ufa76b4bb3c95","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C7a807e462b65","Ud7002ae5a86c",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C481cd737c873","U21769235b28d",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ub786ef7c9e9f","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uf3b5141d73f3","B9c01ce5718d1",-3.0,"").unwrap();
        let _ = crate::mr_put_edge("U430a8328643b","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U72f88cf28226","U499f24158a40",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Bd49e3dac97b0","Uadeb43da4abb",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U83e829a2e822","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C0166be581dd4","U499f24158a40",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Cd172fb3fdc41","U499f24158a40",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U79466f73dc0c","U01814d1ec9ff",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U362d375c067c","C5167c9b3d347",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ue6cc7bfa0efd","Bed5126bc655d",7.0,"").unwrap();
        let _ = crate::mr_put_edge("U8842ed397bb7","C789dceb76123",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uda0a7acaeb90","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U499f24158a40","Ccbd85b8513f3",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U0a227036e790","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Cf77494dc63d7","U38fdca6685ca",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U9a89e0679dec","B0e230e9108dd",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Cd4417a5d718e","Ub93799d9400e",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U7553cc7bb536","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ub01f4ad1b03f","Ud9df8116deba",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U5f7ff9cb9304","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uee0fbe261b7f","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uef7fbf45ef11","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Cb14487d862b3","Uf5096f6ab14e",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ud04c89aaf453","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uaa4e2be7a87a","C588ffef22463",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U0c17798eaab4","C588ffef22463",5.0,"").unwrap();
        let _ = crate::mr_put_edge("Uaa4e2be7a87a","C78d6fac93d00",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ue3b747447a90","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U59abf06369c3","Be2b46c17f1da",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ucb84c094edba","B491d307dfe01",0.0,"").unwrap();
        let _ = crate::mr_put_edge("Uc2bfe7e7308d","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ubeded808a9c0","B7f628ad203b5",-9.0,"").unwrap();
        let _ = crate::mr_put_edge("Uac897fe92894","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U80e22da6d8c4","Ue7a29d5409f2",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U4ba2e4e81c0e","Caa62fc21e191",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ub93799d9400e","Ccae34b3da05e",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U9e42f6dab85a","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U0e6659929c53","C6d52e861b366",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("U38fdca6685ca","C0f834110f700",1.0,"").unwrap();
        let _ = crate::mr_put_edge("B92e4a185c654","U41784ed376c3",1.0,"").unwrap();
        let _ = crate::mr_put_edge("B5a1c1d3d0140","Uc3c31b8a022f",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C6a2263dc469e","Uf2b0a6b1d423",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U9a89e0679dec","Cbce32a9b256a",6.0,"").unwrap();
        let _ = crate::mr_put_edge("Uf5096f6ab14e","C3e84102071d1",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uef7fbf45ef11","C94bb73c10a06",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C4f2dafca724f","U7a8d8324441d",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U4f530cfe771e","B7f628ad203b5",0.0,"").unwrap();
        let _ = crate::mr_put_edge("Ub01f4ad1b03f","B75a44a52fa29",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ubd93205079e9","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U9a2c85753a6d","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U7bd2e29031a4","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ud5f1a29622d1","B7f628ad203b5",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U8676859527f3","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Cbbf2df46955b","U7a8d8324441d",1.0,"").unwrap();
        let _ = crate::mr_put_edge("B5eb4c6be535a","Uad577360d968",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U01814d1ec9ff","C7062e90f7422",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U41784ed376c3","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U99a0f1f7e6ee","C279db553a831",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C15d8dfaceb75","U9e42f6dab85a",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ca0a6aea6c82e","U016217c34c6e",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uc3c31b8a022f","B5a1c1d3d0140",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U35eb26fc07b4","B7f628ad203b5",-2.0,"").unwrap();
        let _ = crate::mr_put_edge("U5f2702cc8ade","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U11456af7d414","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ue40b938f47a4","Cb3c476a45037",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uaa4e2be7a87a","C070e739180d6",8.0,"").unwrap();
        let _ = crate::mr_put_edge("U8a78048d60f7","B8a531802473b",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("U1df3e39ebe59","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U6661263fb410","Ccb7dc40f1513",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U8a78048d60f7","Cb07d467c1c5e",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C789dceb76123","U8842ed397bb7",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uad577360d968","U389f9f24b31c",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U57b6f30fc663","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U2d8ff859cca4","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C54972a5fbc16","U499f24158a40",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U052641f28245","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Bb78026d99388","U9a89e0679dec",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U389f9f24b31c","B25c85fe0df2d",5.0,"").unwrap();
        let _ = crate::mr_put_edge("Uc244d6132650","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U79466f73dc0c","Bad1c69de7837",2.0,"").unwrap();
        let _ = crate::mr_put_edge("U622a649ddf56","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U8a78048d60f7","Ba3c4a280657d",3.0,"").unwrap();
        let _ = crate::mr_put_edge("C6d52e861b366","U21769235b28d",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U1eedef3e4d10","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U8a78048d60f7","C6acd550a4ef3",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U80e22da6d8c4","B60d725feca77",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U1c285703fc63","B63fbe1427d09",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uc5d62a177997","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U8aa2e2623fa5","C7c4d9ca4623e",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U1bcba4fd7175","C6d52e861b366",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("C30e7409c2d5f","U80e22da6d8c4",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ub01f4ad1b03f","B491d307dfe01",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U77f496546efa","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U798f0a5b78f0","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C801f204d0da8","U21769235b28d",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C5782d559baad","U0cd6bd2dde4f",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U41784ed376c3","B92e4a185c654",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U26aca0e369c7","Cb117f464e558",6.0,"").unwrap();
        let _ = crate::mr_put_edge("U704bd6ecde75","Cdd49e516723a",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ucbca544d500f","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ucbd309d6fcc0","B5e7178dd70bb",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Ue2570414501b","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Uf8bf10852d43","B19d70698e3d8",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U8fc7861a79b9","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U5502925dfe14","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C5060d0101429","U362d375c067c",1.0,"").unwrap();
        let _ = crate::mr_put_edge("B253177f84f08","Uf8bf10852d43",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U34252014c05b","Bb1e3630d2f4a",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U80e22da6d8c4","Cb14487d862b3",6.0,"").unwrap();
        let _ = crate::mr_put_edge("U707f9ed34910","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("Cc01e00342d63","U6661263fb410",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C10872dc9b863","U499f24158a40",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U8a78048d60f7","Be29b4af3f7a5",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("U499f24158a40","C4818c4ed20bf",1.0,"").unwrap();
        let _ = crate::mr_put_edge("C3fd1fdebe0e9","U7a8d8324441d",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U11456af7d414","Bad1c69de7837",-2.0,"").unwrap();
        let _ = crate::mr_put_edge("U6a774cf456f7","U000000000000",1.0,"").unwrap();
        let _ = crate::mr_put_edge("U80e22da6d8c4","B45d72e29f004",3.0,"").unwrap();
        let _ = crate::mr_put_edge("U8a78048d60f7","B3b3f2ecde430",-1.0,"").unwrap();
        let _ = crate::mr_put_edge("U1bcba4fd7175","Bc4addf09b79f",3.0,"").unwrap();
    }

    fn delete_testing_edges() {
        let _ = crate::mr_delete_edge("U0cd6bd2dde4f","B7f628ad203b5","");
        let _ = crate::mr_delete_edge("U7c9ce0ac22b7","U000000000000","");
        let _ = crate::mr_delete_edge("U9a2c85753a6d","C070e739180d6","");
        let _ = crate::mr_delete_edge("U1c285703fc63","Bad1c69de7837","");
        let _ = crate::mr_delete_edge("U25982b736535","U000000000000","");
        let _ = crate::mr_delete_edge("U8a78048d60f7","B92e4a185c654","");
        let _ = crate::mr_delete_edge("U663cd1f1e343","U000000000000","");
        let _ = crate::mr_delete_edge("U9a89e0679dec","U000000000000","");
        let _ = crate::mr_delete_edge("U09cf1f359454","B3c467fb437b2","");
        let _ = crate::mr_delete_edge("U5b09928b977a","U000000000000","");
        let _ = crate::mr_delete_edge("U585dfead09c6","C6d52e861b366","");
        let _ = crate::mr_delete_edge("U02fbd7c8df4c","U000000000000","");
        let _ = crate::mr_delete_edge("Uc1158424318a","C78d6fac93d00","");
        let _ = crate::mr_delete_edge("U7a8d8324441d","Cbbf2df46955b","");
        let _ = crate::mr_delete_edge("U4f530cfe771e","B9c01ce5718d1","");
        let _ = crate::mr_delete_edge("U8a78048d60f7","Cd6c9d5cba220","");
        let _ = crate::mr_delete_edge("Cf4b448ef8618","U499f24158a40","");
        let _ = crate::mr_delete_edge("U4d6816b2416e","U000000000000","");
        let _ = crate::mr_delete_edge("U6942e4590e93","U000000000000","");
        let _ = crate::mr_delete_edge("U389f9f24b31c","Cbbf2df46955b","");
        let _ = crate::mr_delete_edge("Ub01f4ad1b03f","B73a44e2bbd44","");
        let _ = crate::mr_delete_edge("Uab16119974a0","U000000000000","");
        let _ = crate::mr_delete_edge("U09cf1f359454","B5a1c1d3d0140","");
        let _ = crate::mr_delete_edge("U8a78048d60f7","U6d2f25cc4264","");
        let _ = crate::mr_delete_edge("U1df3e39ebe59","Bea16f01b8cc5","");
        let _ = crate::mr_delete_edge("C599f6e6f6b64","U26aca0e369c7","");
        let _ = crate::mr_delete_edge("Udb60bbb285ca","U000000000000","");
        let _ = crate::mr_delete_edge("Uf5a84bada7fb","U000000000000","");
        let _ = crate::mr_delete_edge("U79466f73dc0c","B7f628ad203b5","");
        let _ = crate::mr_delete_edge("U6d2f25cc4264","B3c467fb437b2","");
        let _ = crate::mr_delete_edge("Uf59dcd0bc354","U000000000000","");
        let _ = crate::mr_delete_edge("Ucb84c094edba","U000000000000","");
        let _ = crate::mr_delete_edge("Ud7002ae5a86c","B75a44a52fa29","");
        let _ = crate::mr_delete_edge("Uc3c31b8a022f","U000000000000","");
        let _ = crate::mr_delete_edge("U80e22da6d8c4","C6acd550a4ef3","");
        let _ = crate::mr_delete_edge("Uf2b0a6b1d423","B5eb4c6be535a","");
        let _ = crate::mr_delete_edge("B9c01ce5718d1","U499f24158a40","");
        let _ = crate::mr_delete_edge("Ub01f4ad1b03f","U499f24158a40","");
        let _ = crate::mr_delete_edge("U867a75db12ae","U000000000000","");
        let _ = crate::mr_delete_edge("U99a0f1f7e6ee","Bd90a1cf73384","");
        let _ = crate::mr_delete_edge("U6629a0a8ef04","U000000000000","");
        let _ = crate::mr_delete_edge("Ub7f9dfb6a7a5","U000000000000","");
        let _ = crate::mr_delete_edge("Uf31403bd4e20","U000000000000","");
        let _ = crate::mr_delete_edge("U0e214fef4f03","U000000000000","");
        let _ = crate::mr_delete_edge("U0e6659929c53","Cffd169930956","");
        let _ = crate::mr_delete_edge("Cd1c25e32ad21","Ucd424ac24c15","");
        let _ = crate::mr_delete_edge("Uac897fe92894","B9c01ce5718d1","");
        let _ = crate::mr_delete_edge("Bc4addf09b79f","U0cd6bd2dde4f","");
        let _ = crate::mr_delete_edge("U638f5c19326f","B9cade9992fb9","");
        let _ = crate::mr_delete_edge("U290a1ab9d54a","U000000000000","");
        let _ = crate::mr_delete_edge("U3c63a9b6115a","Bad1c69de7837","");
        let _ = crate::mr_delete_edge("U016217c34c6e","U000000000000","");
        let _ = crate::mr_delete_edge("U389f9f24b31c","C6acd550a4ef3","");
        let _ = crate::mr_delete_edge("U9e42f6dab85a","Cbbf2df46955b","");
        let _ = crate::mr_delete_edge("U99a0f1f7e6ee","C4d1d582c53c3","");
        let _ = crate::mr_delete_edge("Be2b46c17f1da","U80e22da6d8c4","");
        let _ = crate::mr_delete_edge("B5e7178dd70bb","Ucbd309d6fcc0","");
        let _ = crate::mr_delete_edge("U7a8d8324441d","U1c285703fc63","");
        let _ = crate::mr_delete_edge("C4893c40e481d","Udece0afd9a8b","");
        let _ = crate::mr_delete_edge("U9e42f6dab85a","B3c467fb437b2","");
        let _ = crate::mr_delete_edge("U8842ed397bb7","U000000000000","");
        let _ = crate::mr_delete_edge("Ue70d59cc8e3f","B9c01ce5718d1","");
        let _ = crate::mr_delete_edge("U5c827d7de115","U000000000000","");
        let _ = crate::mr_delete_edge("Ue94281e36fe8","U000000000000","");
        let _ = crate::mr_delete_edge("U8a78048d60f7","Bdf39d0e1daf5","");
        let _ = crate::mr_delete_edge("U18a178de1dfb","B70df5dbab8c3","");
        let _ = crate::mr_delete_edge("U4f530cfe771e","U000000000000","");
        let _ = crate::mr_delete_edge("Uad577360d968","B5eb4c6be535a","");
        let _ = crate::mr_delete_edge("U526f361717a8","Cee9901f0f22c","");
        let _ = crate::mr_delete_edge("Uccc3c7395af6","U000000000000","");
        let _ = crate::mr_delete_edge("C2bbd63b00224","U80e22da6d8c4","");
        let _ = crate::mr_delete_edge("Cb3c476a45037","Ue40b938f47a4","");
        let _ = crate::mr_delete_edge("U1c634fdd7c82","U000000000000","");
        let _ = crate::mr_delete_edge("C22e1102411ce","U6661263fb410","");
        let _ = crate::mr_delete_edge("U57b6f30fc663","Bed5126bc655d","");
        let _ = crate::mr_delete_edge("U6661263fb410","Cf92f90725ffc","");
        let _ = crate::mr_delete_edge("Uef7fbf45ef11","C2bbd63b00224","");
        let _ = crate::mr_delete_edge("U09cf1f359454","Ba5d64165e5d5","");
        let _ = crate::mr_delete_edge("Ub01f4ad1b03f","U79466f73dc0c","");
        let _ = crate::mr_delete_edge("U40096feaa029","U000000000000","");
        let _ = crate::mr_delete_edge("U09cf1f359454","B5eb4c6be535a","");
        let _ = crate::mr_delete_edge("U8a78048d60f7","B499bfc56e77b","");
        let _ = crate::mr_delete_edge("U3c63a9b6115a","Cf92f90725ffc","");
        let _ = crate::mr_delete_edge("U9ce5721e93cf","U000000000000","");
        let _ = crate::mr_delete_edge("Ud04c89aaf453","B4f14b223b56d","");
        let _ = crate::mr_delete_edge("Ue7a29d5409f2","Udece0afd9a8b","");
        let _ = crate::mr_delete_edge("U38fdca6685ca","Cf77494dc63d7","");
        let _ = crate::mr_delete_edge("U83282a51b600","Be2b46c17f1da","");
        let _ = crate::mr_delete_edge("U83e829a2e822","B7f628ad203b5","");
        let _ = crate::mr_delete_edge("Bc896788cd2ef","U1bcba4fd7175","");
        let _ = crate::mr_delete_edge("Uf2b0a6b1d423","C67e4476fda28","");
        let _ = crate::mr_delete_edge("C9028c7415403","Udece0afd9a8b","");
        let _ = crate::mr_delete_edge("U01814d1ec9ff","U499f24158a40","");
        let _ = crate::mr_delete_edge("Uadeb43da4abb","B0e230e9108dd","");
        let _ = crate::mr_delete_edge("U1bcba4fd7175","C264c56d501db","");
        let _ = crate::mr_delete_edge("U8a78048d60f7","B73a44e2bbd44","");
        let _ = crate::mr_delete_edge("Ud982a6dee46f","Be7145faf15cb","");
        let _ = crate::mr_delete_edge("B0a87a669fc28","U34252014c05b","");
        let _ = crate::mr_delete_edge("U0e6659929c53","Cb967536095de","");
        let _ = crate::mr_delete_edge("Ucb37b247402a","U000000000000","");
        let _ = crate::mr_delete_edge("C0f834110f700","U38fdca6685ca","");
        let _ = crate::mr_delete_edge("U72f88cf28226","Cb11edc3d0bc7","");
        let _ = crate::mr_delete_edge("U499f24158a40","C0166be581dd4","");
        let _ = crate::mr_delete_edge("U5d9b4e4a7baf","U000000000000","");
        let _ = crate::mr_delete_edge("U526f361717a8","C52d41a9ad558","");
        let _ = crate::mr_delete_edge("Ue7a29d5409f2","Cb76829a425d9","");
        let _ = crate::mr_delete_edge("U499f24158a40","Cf4b448ef8618","");
        let _ = crate::mr_delete_edge("U48dcd166b0bd","U000000000000","");
        let _ = crate::mr_delete_edge("Uadeb43da4abb","C30e7409c2d5f","");
        let _ = crate::mr_delete_edge("U05e4396e2382","B7f628ad203b5","");
        let _ = crate::mr_delete_edge("Uf3b5141d73f3","U000000000000","");
        let _ = crate::mr_delete_edge("U8a78048d60f7","Cb11edc3d0bc7","");
        let _ = crate::mr_delete_edge("U18a178de1dfb","B1533941e2773","");
        let _ = crate::mr_delete_edge("B506fff6cfc22","Ub7f9dfb6a7a5","");
        let _ = crate::mr_delete_edge("Uad577360d968","C2bbd63b00224","");
        let _ = crate::mr_delete_edge("Uda5b03b660d7","U000000000000","");
        let _ = crate::mr_delete_edge("U7a8d8324441d","C4f2dafca724f","");
        let _ = crate::mr_delete_edge("Ucb9952d31a9e","U000000000000","");
        let _ = crate::mr_delete_edge("U8a78048d60f7","Bd7a8bfcf3337","");
        let _ = crate::mr_delete_edge("C1ccb4354d684","Ue202d5b01f8d","");
        let _ = crate::mr_delete_edge("Ud5b22ebf52f2","Cd6c9d5cba220","");
        let _ = crate::mr_delete_edge("U8a78048d60f7","Ba5d64165e5d5","");
        let _ = crate::mr_delete_edge("U0da9e22a248b","U000000000000","");
        let _ = crate::mr_delete_edge("Uab20c65d180d","U000000000000","");
        let _ = crate::mr_delete_edge("Uf5096f6ab14e","C6aebafa4fe8e","");
        let _ = crate::mr_delete_edge("Uef7fbf45ef11","C588ffef22463","");
        let _ = crate::mr_delete_edge("Ccae34b3da05e","Ub93799d9400e","");
        let _ = crate::mr_delete_edge("U8a78048d60f7","B9c01ce5718d1","");
        let _ = crate::mr_delete_edge("Uc35c445325f5","B75a44a52fa29","");
        let _ = crate::mr_delete_edge("U362d375c067c","Ce06bda6030fe","");
        let _ = crate::mr_delete_edge("Uaa4e2be7a87a","Cfdde53c79a2d","");
        let _ = crate::mr_delete_edge("U09cf1f359454","B75a44a52fa29","");
        let _ = crate::mr_delete_edge("Bb5f87c1621d5","Ub01f4ad1b03f","");
        let _ = crate::mr_delete_edge("U016217c34c6e","B3c467fb437b2","");
        let _ = crate::mr_delete_edge("U9a2c85753a6d","Udece0afd9a8b","");
        let _ = crate::mr_delete_edge("U6d2f25cc4264","B63fbe1427d09","");
        let _ = crate::mr_delete_edge("Ub01f4ad1b03f","C5782d559baad","");
        let _ = crate::mr_delete_edge("C3b855f713d19","U704bd6ecde75","");
        let _ = crate::mr_delete_edge("U016217c34c6e","Cb76829a425d9","");
        let _ = crate::mr_delete_edge("Ub0205d5d96d0","U000000000000","");
        let _ = crate::mr_delete_edge("U499f24158a40","Ba3c4a280657d","");
        let _ = crate::mr_delete_edge("U0c17798eaab4","Udece0afd9a8b","");
        let _ = crate::mr_delete_edge("Ud7002ae5a86c","U000000000000","");
        let _ = crate::mr_delete_edge("U35eb26fc07b4","U000000000000","");
        let _ = crate::mr_delete_edge("Uc1158424318a","Bdf39d0e1daf5","");
        let _ = crate::mr_delete_edge("U96a8bbfce56f","U000000000000","");
        let _ = crate::mr_delete_edge("C588ffef22463","Uef7fbf45ef11","");
        let _ = crate::mr_delete_edge("U72f88cf28226","B3f6f837bc345","");
        let _ = crate::mr_delete_edge("Ba3c4a280657d","U499f24158a40","");
        let _ = crate::mr_delete_edge("Ua9d9d5da3948","U000000000000","");
        let _ = crate::mr_delete_edge("U8a78048d60f7","Bd90a1cf73384","");
        let _ = crate::mr_delete_edge("U638f5c19326f","B9c01ce5718d1","");
        let _ = crate::mr_delete_edge("U80e22da6d8c4","U000000000000","");
        let _ = crate::mr_delete_edge("Udf6d8127c2c6","U000000000000","");
        let _ = crate::mr_delete_edge("U362d375c067c","U000000000000","");
        let _ = crate::mr_delete_edge("U83282a51b600","C9462ca240ceb","");
        let _ = crate::mr_delete_edge("U638f5c19326f","U000000000000","");
        let _ = crate::mr_delete_edge("U3116d27854ab","U000000000000","");
        let _ = crate::mr_delete_edge("U499f24158a40","C54972a5fbc16","");
        let _ = crate::mr_delete_edge("Ub93799d9400e","B9c01ce5718d1","");
        let _ = crate::mr_delete_edge("U9e42f6dab85a","C15d8dfaceb75","");
        let _ = crate::mr_delete_edge("U1bcba4fd7175","Be2b46c17f1da","");
        let _ = crate::mr_delete_edge("B8a531802473b","U016217c34c6e","");
        let _ = crate::mr_delete_edge("U01814d1ec9ff","Bb78026d99388","");
        let _ = crate::mr_delete_edge("Ue7a29d5409f2","C4893c40e481d","");
        let _ = crate::mr_delete_edge("Cb11edc3d0bc7","U8a78048d60f7","");
        let _ = crate::mr_delete_edge("Ub01f4ad1b03f","Bb1e3630d2f4a","");
        let _ = crate::mr_delete_edge("U0cd6bd2dde4f","B92e4a185c654","");
        let _ = crate::mr_delete_edge("U09cf1f359454","B45d72e29f004","");
        let _ = crate::mr_delete_edge("Cab47a458295f","U6d2f25cc4264","");
        let _ = crate::mr_delete_edge("U7dd2b82154e0","U000000000000","");
        let _ = crate::mr_delete_edge("U1f348902b446","U000000000000","");
        let _ = crate::mr_delete_edge("Uad577360d968","U000000000000","");
        let _ = crate::mr_delete_edge("U861750348e9f","U000000000000","");
        let _ = crate::mr_delete_edge("Ue55b928fa8dd","Bed5126bc655d","");
        let _ = crate::mr_delete_edge("U016217c34c6e","U9a89e0679dec","");
        let _ = crate::mr_delete_edge("Uc35c445325f5","U000000000000","");
        let _ = crate::mr_delete_edge("U09cf1f359454","U8a78048d60f7","");
        let _ = crate::mr_delete_edge("C6aebafa4fe8e","U9a2c85753a6d","");
        let _ = crate::mr_delete_edge("Ucdffb8ab5145","Cf8fb8c05c116","");
        let _ = crate::mr_delete_edge("U0cd6bd2dde4f","B9c01ce5718d1","");
        let _ = crate::mr_delete_edge("U59abf06369c3","Cda989f4b466d","");
        let _ = crate::mr_delete_edge("B4f00e7813add","U09cf1f359454","");
        let _ = crate::mr_delete_edge("U8a78048d60f7","B75a44a52fa29","");
        let _ = crate::mr_delete_edge("U80e22da6d8c4","U0c17798eaab4","");
        let _ = crate::mr_delete_edge("Ub01f4ad1b03f","U09cf1f359454","");
        let _ = crate::mr_delete_edge("U21769235b28d","C801f204d0da8","");
        let _ = crate::mr_delete_edge("Uddd01c7863e9","U000000000000","");
        let _ = crate::mr_delete_edge("U9a2c85753a6d","B3c467fb437b2","");
        let _ = crate::mr_delete_edge("U06a4bdf76bf7","U000000000000","");
        let _ = crate::mr_delete_edge("U43dcf522b4dd","B3b3f2ecde430","");
        let _ = crate::mr_delete_edge("C264c56d501db","U1bcba4fd7175","");
        let _ = crate::mr_delete_edge("Ua4041a93bdf4","B9c01ce5718d1","");
        let _ = crate::mr_delete_edge("Uc3c31b8a022f","B45d72e29f004","");
        let _ = crate::mr_delete_edge("Uaaf5341090c6","U000000000000","");
        let _ = crate::mr_delete_edge("Uf2b0a6b1d423","C399b6349ab02","");
        let _ = crate::mr_delete_edge("U45578f837ab8","U000000000000","");
        let _ = crate::mr_delete_edge("Ub01f4ad1b03f","U6d2f25cc4264","");
        let _ = crate::mr_delete_edge("Uf5096f6ab14e","B3b3f2ecde430","");
        let _ = crate::mr_delete_edge("Ub01f4ad1b03f","U8a78048d60f7","");
        let _ = crate::mr_delete_edge("Ub01f4ad1b03f","B5eb4c6be535a","");
        let _ = crate::mr_delete_edge("Uc1158424318a","Cfdde53c79a2d","");
        let _ = crate::mr_delete_edge("Ub192fb5e4fee","U000000000000","");
        let _ = crate::mr_delete_edge("U7eaa146a4793","U000000000000","");
        let _ = crate::mr_delete_edge("Udece0afd9a8b","Uadeb43da4abb","");
        let _ = crate::mr_delete_edge("U6d2f25cc4264","Bdf39d0e1daf5","");
        let _ = crate::mr_delete_edge("U96a7841bc98d","U000000000000","");
        let _ = crate::mr_delete_edge("U7ac570b5840f","U000000000000","");
        let _ = crate::mr_delete_edge("U80e22da6d8c4","Cbbf2df46955b","");
        let _ = crate::mr_delete_edge("U7399d6656581","U000000000000","");
        let _ = crate::mr_delete_edge("Ud7186ef65120","U000000000000","");
        let _ = crate::mr_delete_edge("U9a2c85753a6d","C78ad459d3b81","");
        let _ = crate::mr_delete_edge("Ub01f4ad1b03f","B5a1c1d3d0140","");
        let _ = crate::mr_delete_edge("U526c52711601","U000000000000","");
        let _ = crate::mr_delete_edge("U8a78048d60f7","B25c85fe0df2d","");
        let _ = crate::mr_delete_edge("Uc1158424318a","C6acd550a4ef3","");
        let _ = crate::mr_delete_edge("B310b66ab31fb","U6d2f25cc4264","");
        let _ = crate::mr_delete_edge("U499f24158a40","C4b2b6fd8fa9a","");
        let _ = crate::mr_delete_edge("B70df5dbab8c3","U09cf1f359454","");
        let _ = crate::mr_delete_edge("U1bcba4fd7175","U09cf1f359454","");
        let _ = crate::mr_delete_edge("U18a178de1dfb","B75a44a52fa29","");
        let _ = crate::mr_delete_edge("Uadeb43da4abb","C9462ca240ceb","");
        let _ = crate::mr_delete_edge("U9a89e0679dec","Bb78026d99388","");
        let _ = crate::mr_delete_edge("U89a6e30efb07","U000000000000","");
        let _ = crate::mr_delete_edge("U8a78048d60f7","B491d307dfe01","");
        let _ = crate::mr_delete_edge("C7c4d9ca4623e","U8aa2e2623fa5","");
        let _ = crate::mr_delete_edge("Ub901d5e0edca","U000000000000","");
        let _ = crate::mr_delete_edge("U01814d1ec9ff","C1c86825bd597","");
        let _ = crate::mr_delete_edge("Udece0afd9a8b","C357396896bd0","");
        let _ = crate::mr_delete_edge("Ub01f4ad1b03f","B4f00e7813add","");
        let _ = crate::mr_delete_edge("U1c285703fc63","U9e42f6dab85a","");
        let _ = crate::mr_delete_edge("Ua50dd76e5a75","U000000000000","");
        let _ = crate::mr_delete_edge("U1e41b5f3adff","B310b66ab31fb","");
        let _ = crate::mr_delete_edge("Cc2b3069cbe5d","Ub01f4ad1b03f","");
        let _ = crate::mr_delete_edge("Uaa4e2be7a87a","Uadeb43da4abb","");
        let _ = crate::mr_delete_edge("Ucd424ac24c15","U000000000000","");
        let _ = crate::mr_delete_edge("U431131a166be","U000000000000","");
        let _ = crate::mr_delete_edge("U59abf06369c3","B7f628ad203b5","");
        let _ = crate::mr_delete_edge("U146915ad287e","U000000000000","");
        let _ = crate::mr_delete_edge("U1bcba4fd7175","B45d72e29f004","");
        let _ = crate::mr_delete_edge("Ud5f1a29622d1","U000000000000","");
        let _ = crate::mr_delete_edge("U05e4396e2382","Bad1c69de7837","");
        let _ = crate::mr_delete_edge("Cd795a41fe71d","U362d375c067c","");
        let _ = crate::mr_delete_edge("U389f9f24b31c","U000000000000","");
        let _ = crate::mr_delete_edge("U72f88cf28226","B310b66ab31fb","");
        let _ = crate::mr_delete_edge("B4f14b223b56d","Ud04c89aaf453","");
        let _ = crate::mr_delete_edge("Uf6ce05bc4e5a","U000000000000","");
        let _ = crate::mr_delete_edge("U1e41b5f3adff","U6d2f25cc4264","");
        let _ = crate::mr_delete_edge("U83e829a2e822","B0e230e9108dd","");
        let _ = crate::mr_delete_edge("Ucbd309d6fcc0","U000000000000","");
        let _ = crate::mr_delete_edge("U8f0839032839","U000000000000","");
        let _ = crate::mr_delete_edge("Uf2b0a6b1d423","C6a2263dc469e","");
        let _ = crate::mr_delete_edge("Ueb1e69384e4e","U000000000000","");
        let _ = crate::mr_delete_edge("C89c123f7bcf5","U8842ed397bb7","");
        let _ = crate::mr_delete_edge("Ufb826ea158e5","U000000000000","");
        let _ = crate::mr_delete_edge("U26aca0e369c7","C4893c40e481d","");
        let _ = crate::mr_delete_edge("Ue7a29d5409f2","Uaa4e2be7a87a","");
        let _ = crate::mr_delete_edge("Uf5096f6ab14e","C4893c40e481d","");
        let _ = crate::mr_delete_edge("U18a178de1dfb","B3f6f837bc345","");
        let _ = crate::mr_delete_edge("U6d2f25cc4264","C25639690ee57","");
        let _ = crate::mr_delete_edge("U6d2f25cc4264","Ud9df8116deba","");
        let _ = crate::mr_delete_edge("Ca8ceac412e6f","U4ba2e4e81c0e","");
        let _ = crate::mr_delete_edge("Be29b4af3f7a5","Uc35c445325f5","");
        let _ = crate::mr_delete_edge("U1188b2dfb294","U000000000000","");
        let _ = crate::mr_delete_edge("U01814d1ec9ff","U02fbd7c8df4c","");
        let _ = crate::mr_delete_edge("Cb07d467c1c5e","U8a78048d60f7","");
        let _ = crate::mr_delete_edge("U8aa2e2623fa5","B9c01ce5718d1","");
        let _ = crate::mr_delete_edge("Ub01f4ad1b03f","B3b3f2ecde430","");
        let _ = crate::mr_delete_edge("U88e719e6257d","U000000000000","");
        let _ = crate::mr_delete_edge("U8a78048d60f7","Cf4b448ef8618","");
        let _ = crate::mr_delete_edge("U4c619411e5de","U000000000000","");
        let _ = crate::mr_delete_edge("C9a2135edf7ff","U83282a51b600","");
        let _ = crate::mr_delete_edge("Ub01f4ad1b03f","B19ea554faf29","");
        let _ = crate::mr_delete_edge("Ba5d64165e5d5","U1e41b5f3adff","");
        let _ = crate::mr_delete_edge("Uf5096f6ab14e","U000000000000","");
        let _ = crate::mr_delete_edge("Ucfe743b8deb1","U000000000000","");
        let _ = crate::mr_delete_edge("U9605bd4d1218","B75a44a52fa29","");
        let _ = crate::mr_delete_edge("B499bfc56e77b","Uc1158424318a","");
        let _ = crate::mr_delete_edge("U1c285703fc63","Cd59e6cd7e104","");
        let _ = crate::mr_delete_edge("U83e829a2e822","Be2b46c17f1da","");
        let _ = crate::mr_delete_edge("U8a78048d60f7","B45d72e29f004","");
        let _ = crate::mr_delete_edge("U499f24158a40","U000000000000","");
        let _ = crate::mr_delete_edge("Cb117f464e558","U26aca0e369c7","");
        let _ = crate::mr_delete_edge("U4ba2e4e81c0e","B7f628ad203b5","");
        let _ = crate::mr_delete_edge("U18a178de1dfb","B19ea554faf29","");
        let _ = crate::mr_delete_edge("Cfd59a206c07d","U99a0f1f7e6ee","");
        let _ = crate::mr_delete_edge("C8ece5c618ac1","U21769235b28d","");
        let _ = crate::mr_delete_edge("Uadeb43da4abb","Cc9f863ff681b","");
        let _ = crate::mr_delete_edge("Ubd3c556b8a25","U000000000000","");
        let _ = crate::mr_delete_edge("U389f9f24b31c","Cdcddfb230cb5","");
        let _ = crate::mr_delete_edge("Uc1158424318a","Cc9f863ff681b","");
        let _ = crate::mr_delete_edge("U26aca0e369c7","C6acd550a4ef3","");
        let _ = crate::mr_delete_edge("C8c753f46c014","U8842ed397bb7","");
        let _ = crate::mr_delete_edge("C78d6fac93d00","Uc1158424318a","");
        let _ = crate::mr_delete_edge("U9a2c85753a6d","C357396896bd0","");
        let _ = crate::mr_delete_edge("U389f9f24b31c","Cd59e6cd7e104","");
        let _ = crate::mr_delete_edge("U8c33fbcc06d7","U000000000000","");
        let _ = crate::mr_delete_edge("Bf3a0a1165271","U9a89e0679dec","");
        let _ = crate::mr_delete_edge("U09cf1f359454","B70df5dbab8c3","");
        let _ = crate::mr_delete_edge("Cb967536095de","U0e6659929c53","");
        let _ = crate::mr_delete_edge("C0b19d314485e","Uaa4e2be7a87a","");
        let _ = crate::mr_delete_edge("U09cf1f359454","Bc896788cd2ef","");
        let _ = crate::mr_delete_edge("Uc35c445325f5","B9c01ce5718d1","");
        let _ = crate::mr_delete_edge("U01814d1ec9ff","B9c01ce5718d1","");
        let _ = crate::mr_delete_edge("C25639690ee57","U6d2f25cc4264","");
        let _ = crate::mr_delete_edge("Ue202d5b01f8d","U000000000000","");
        let _ = crate::mr_delete_edge("U362d375c067c","Bad1c69de7837","");
        let _ = crate::mr_delete_edge("U1c285703fc63","C67e4476fda28","");
        let _ = crate::mr_delete_edge("Ue7a29d5409f2","U000000000000","");
        let _ = crate::mr_delete_edge("U8a78048d60f7","B60d725feca77","");
        let _ = crate::mr_delete_edge("U8a78048d60f7","Bfefe4e25c870","");
        let _ = crate::mr_delete_edge("Uc4f728b0d87f","U000000000000","");
        let _ = crate::mr_delete_edge("U9a2c85753a6d","Cdcddfb230cb5","");
        let _ = crate::mr_delete_edge("Uf5096f6ab14e","Cb14487d862b3","");
        let _ = crate::mr_delete_edge("U682c3380036f","C7986cd8a648a","");
        let _ = crate::mr_delete_edge("U02fbd7c8df4c","Bd7a8bfcf3337","");
        let _ = crate::mr_delete_edge("U7a8d8324441d","Cbbf2df46955b","");
        let _ = crate::mr_delete_edge("U8a78048d60f7","U6240251593cd","");
        let _ = crate::mr_delete_edge("U499f24158a40","C8d80016b8292","");
        let _ = crate::mr_delete_edge("Uc35c445325f5","B8a531802473b","");
        let _ = crate::mr_delete_edge("U704bd6ecde75","B9c01ce5718d1","");
        let _ = crate::mr_delete_edge("U77f496546efa","B9c01ce5718d1","");
        let _ = crate::mr_delete_edge("U6d2f25cc4264","B7f628ad203b5","");
        let _ = crate::mr_delete_edge("Ub01f4ad1b03f","B10d3f548efc4","");
        let _ = crate::mr_delete_edge("U7a8d8324441d","Cd06fea6a395f","");
        let _ = crate::mr_delete_edge("U682c3380036f","U000000000000","");
        let _ = crate::mr_delete_edge("U36055bb45e5c","U000000000000","");
        let _ = crate::mr_delete_edge("U7cdd7999301e","B7f628ad203b5","");
        let _ = crate::mr_delete_edge("U526f361717a8","Cf40e8fb326bc","");
        let _ = crate::mr_delete_edge("U946ae258c4b5","U000000000000","");
        let _ = crate::mr_delete_edge("B944097cdd968","Ue40b938f47a4","");
        let _ = crate::mr_delete_edge("U2f08dff8dbdb","U000000000000","");
        let _ = crate::mr_delete_edge("Ub01f4ad1b03f","B3f6f837bc345","");
        let _ = crate::mr_delete_edge("U6661263fb410","Cc01e00342d63","");
        let _ = crate::mr_delete_edge("U80e22da6d8c4","Cb76829a425d9","");
        let _ = crate::mr_delete_edge("Ccb7dc40f1513","U6661263fb410","");
        let _ = crate::mr_delete_edge("U83282a51b600","C9a2135edf7ff","");
        let _ = crate::mr_delete_edge("Cb76829a425d9","Ue7a29d5409f2","");
        let _ = crate::mr_delete_edge("B45d72e29f004","U26aca0e369c7","");
        let _ = crate::mr_delete_edge("Ue6cc7bfa0efd","B5e7178dd70bb","");
        let _ = crate::mr_delete_edge("Uac897fe92894","Be2b46c17f1da","");
        let _ = crate::mr_delete_edge("B73a44e2bbd44","U8a78048d60f7","");
        let _ = crate::mr_delete_edge("Ue7a29d5409f2","C399b6349ab02","");
        let _ = crate::mr_delete_edge("Cfa08a39f9bb9","Ubebfe0c8fc29","");
        let _ = crate::mr_delete_edge("Cdcddfb230cb5","Udece0afd9a8b","");
        let _ = crate::mr_delete_edge("Ub01f4ad1b03f","Bb5f87c1621d5","");
        let _ = crate::mr_delete_edge("U7a8d8324441d","C78d6fac93d00","");
        let _ = crate::mr_delete_edge("U18a178de1dfb","U000000000000","");
        let _ = crate::mr_delete_edge("U2cd96f1b2ea6","U000000000000","");
        let _ = crate::mr_delete_edge("Ub01f4ad1b03f","B3c467fb437b2","");
        let _ = crate::mr_delete_edge("C0cd490b5fb6a","Uad577360d968","");
        let _ = crate::mr_delete_edge("U80e22da6d8c4","Be2b46c17f1da","");
        let _ = crate::mr_delete_edge("U0f63ee3db59b","U000000000000","");
        let _ = crate::mr_delete_edge("U09cf1f359454","B499bfc56e77b","");
        let _ = crate::mr_delete_edge("U7a975ca7e0b0","U000000000000","");
        let _ = crate::mr_delete_edge("C2cb023b6bcef","Ucb84c094edba","");
        let _ = crate::mr_delete_edge("U016217c34c6e","C4e0db8dec53e","");
        let _ = crate::mr_delete_edge("Cc931cd2de143","Ud7002ae5a86c","");
        let _ = crate::mr_delete_edge("U0c17798eaab4","C4893c40e481d","");
        let _ = crate::mr_delete_edge("U1c285703fc63","U016217c34c6e","");
        let _ = crate::mr_delete_edge("U4a82930ca419","U000000000000","");
        let _ = crate::mr_delete_edge("U682c3380036f","U6240251593cd","");
        let _ = crate::mr_delete_edge("U2a62e985bcd5","U000000000000","");
        let _ = crate::mr_delete_edge("U1e6a314ef612","U000000000000","");
        let _ = crate::mr_delete_edge("Uab766aeb8fd2","U000000000000","");
        let _ = crate::mr_delete_edge("U18a178de1dfb","B4f00e7813add","");
        let _ = crate::mr_delete_edge("Uc4ebbce44401","U000000000000","");
        let _ = crate::mr_delete_edge("Ud7002ae5a86c","Cc931cd2de143","");
        let _ = crate::mr_delete_edge("Ue5c10787d0db","U000000000000","");
        let _ = crate::mr_delete_edge("U499f24158a40","B79efabc4d8bf","");
        let _ = crate::mr_delete_edge("U044c5bf57a97","U000000000000","");
        let _ = crate::mr_delete_edge("U99deecf5a281","U000000000000","");
        let _ = crate::mr_delete_edge("U9605bd4d1218","U000000000000","");
        let _ = crate::mr_delete_edge("U3de789cac826","B9c01ce5718d1","");
        let _ = crate::mr_delete_edge("U7a8d8324441d","C888c86d096d0","");
        let _ = crate::mr_delete_edge("U5f148383594f","U000000000000","");
        let _ = crate::mr_delete_edge("U499f24158a40","C10872dc9b863","");
        let _ = crate::mr_delete_edge("B0e230e9108dd","U9a89e0679dec","");
        let _ = crate::mr_delete_edge("Ufec0de2f341d","U000000000000","");
        let _ = crate::mr_delete_edge("U8a78048d60f7","C2e31b4b1658f","");
        let _ = crate::mr_delete_edge("U09cf1f359454","B25c85fe0df2d","");
        let _ = crate::mr_delete_edge("Uc3db248a6e7f","U000000000000","");
        let _ = crate::mr_delete_edge("U09cf1f359454","C81f3f954b643","");
        let _ = crate::mr_delete_edge("U6eab54d64086","U000000000000","");
        let _ = crate::mr_delete_edge("C0a576fc389d9","U1bcba4fd7175","");
        let _ = crate::mr_delete_edge("U4a6d6f193ae0","U000000000000","");
        let _ = crate::mr_delete_edge("U6d2f25cc4264","B3f6f837bc345","");
        let _ = crate::mr_delete_edge("U01814d1ec9ff","C6d52e861b366","");
        let _ = crate::mr_delete_edge("U362d375c067c","Cd795a41fe71d","");
        let _ = crate::mr_delete_edge("Uc676bd7563ec","U000000000000","");
        let _ = crate::mr_delete_edge("U6d2f25cc4264","B3b3f2ecde430","");
        let _ = crate::mr_delete_edge("U585dfead09c6","B9c01ce5718d1","");
        let _ = crate::mr_delete_edge("Cfd47f43ac9cf","U704bd6ecde75","");
        let _ = crate::mr_delete_edge("U72f88cf28226","Cd6c9d5cba220","");
        let _ = crate::mr_delete_edge("Cdd49e516723a","U704bd6ecde75","");
        let _ = crate::mr_delete_edge("U26aca0e369c7","Be2b46c17f1da","");
        let _ = crate::mr_delete_edge("U6249d53929c4","U000000000000","");
        let _ = crate::mr_delete_edge("Uad577360d968","C588ffef22463","");
        let _ = crate::mr_delete_edge("U8a78048d60f7","B3c467fb437b2","");
        let _ = crate::mr_delete_edge("Bf34ee3bfc12b","U6240251593cd","");
        let _ = crate::mr_delete_edge("Uf2b0a6b1d423","Bb78026d99388","");
        let _ = crate::mr_delete_edge("Ue202d5b01f8d","B9c01ce5718d1","");
        let _ = crate::mr_delete_edge("U6d2f25cc4264","B310b66ab31fb","");
        let _ = crate::mr_delete_edge("U35eb26fc07b4","C90290100a953","");
        let _ = crate::mr_delete_edge("Cc9f863ff681b","Uc1158424318a","");
        let _ = crate::mr_delete_edge("Uf5ee43a1b729","C9218f86f6286","");
        let _ = crate::mr_delete_edge("C888c86d096d0","U7a8d8324441d","");
        let _ = crate::mr_delete_edge("U499f24158a40","Bfefe4e25c870","");
        let _ = crate::mr_delete_edge("U499f24158a40","C6f84810d3cd9","");
        let _ = crate::mr_delete_edge("Cd6c9d5cba220","Ud5b22ebf52f2","");
        let _ = crate::mr_delete_edge("U99a0f1f7e6ee","C96bdee4f11e2","");
        let _ = crate::mr_delete_edge("U4a82930ca419","C2d9ab331aed7","");
        let _ = crate::mr_delete_edge("C4818c4ed20bf","U499f24158a40","");
        let _ = crate::mr_delete_edge("U585dfead09c6","U000000000000","");
        let _ = crate::mr_delete_edge("Ucd424ac24c15","Cd1c25e32ad21","");
        let _ = crate::mr_delete_edge("U389f9f24b31c","Bad1c69de7837","");
        let _ = crate::mr_delete_edge("U20d01ad4d96b","U000000000000","");
        let _ = crate::mr_delete_edge("Ue7a29d5409f2","Cfdde53c79a2d","");
        let _ = crate::mr_delete_edge("U5ef3d593e46e","U000000000000","");
        let _ = crate::mr_delete_edge("U7382ac807a4f","U000000000000","");
        let _ = crate::mr_delete_edge("U88137a4bf483","U000000000000","");
        let _ = crate::mr_delete_edge("U5c827d7de115","B69723edfec8a","");
        let _ = crate::mr_delete_edge("U1bcba4fd7175","Cd4417a5d718e","");
        let _ = crate::mr_delete_edge("Ue202d5b01f8d","C1ccb4354d684","");
        let _ = crate::mr_delete_edge("U6d2f25cc4264","B9c01ce5718d1","");
        let _ = crate::mr_delete_edge("Udece0afd9a8b","Cdcddfb230cb5","");
        let _ = crate::mr_delete_edge("Ufca294ffe3a5","U000000000000","");
        let _ = crate::mr_delete_edge("C81f3f954b643","U09cf1f359454","");
        let _ = crate::mr_delete_edge("U02fbd7c8df4c","B75a44a52fa29","");
        let _ = crate::mr_delete_edge("U049bf307d470","U000000000000","");
        let _ = crate::mr_delete_edge("U1c285703fc63","C30e7409c2d5f","");
        let _ = crate::mr_delete_edge("U8a78048d60f7","Be5bb2f3d56cb","");
        let _ = crate::mr_delete_edge("U4d82230c274a","U000000000000","");
        let _ = crate::mr_delete_edge("B10d3f548efc4","U99a0f1f7e6ee","");
        let _ = crate::mr_delete_edge("Uc3c31b8a022f","B3c467fb437b2","");
        let _ = crate::mr_delete_edge("C90290100a953","U35eb26fc07b4","");
        let _ = crate::mr_delete_edge("Uadeb43da4abb","U000000000000","");
        let _ = crate::mr_delete_edge("U18a178de1dfb","B310b66ab31fb","");
        let _ = crate::mr_delete_edge("U35eb26fc07b4","Be2b46c17f1da","");
        let _ = crate::mr_delete_edge("Ccbd85b8513f3","U499f24158a40","");
        let _ = crate::mr_delete_edge("U5ee57577b2bd","U000000000000","");
        let _ = crate::mr_delete_edge("U1bcba4fd7175","Bc896788cd2ef","");
        let _ = crate::mr_delete_edge("U0cd6bd2dde4f","C7062e90f7422","");
        let _ = crate::mr_delete_edge("U6d2f25cc4264","Be2b46c17f1da","");
        let _ = crate::mr_delete_edge("C4d1d582c53c3","U99a0f1f7e6ee","");
        let _ = crate::mr_delete_edge("U11722d2113bf","U000000000000","");
        let _ = crate::mr_delete_edge("U59abf06369c3","Cb117f464e558","");
        let _ = crate::mr_delete_edge("B491d307dfe01","U499f24158a40","");
        let _ = crate::mr_delete_edge("B25c85fe0df2d","Uef7fbf45ef11","");
        let _ = crate::mr_delete_edge("Bdf39d0e1daf5","Uc1158424318a","");
        let _ = crate::mr_delete_edge("U9a2c85753a6d","C3e84102071d1","");
        let _ = crate::mr_delete_edge("U2371cf61799b","U000000000000","");
        let _ = crate::mr_delete_edge("U8a78048d60f7","B63fbe1427d09","");
        let _ = crate::mr_delete_edge("U8a78048d60f7","Cd5983133fb67","");
        let _ = crate::mr_delete_edge("Cc616eded7a99","U0f63ee3db59b","");
        let _ = crate::mr_delete_edge("U34252014c05b","B19ea554faf29","");
        let _ = crate::mr_delete_edge("U6622a635b181","U000000000000","");
        let _ = crate::mr_delete_edge("U0f63ee3db59b","B9c01ce5718d1","");
        let _ = crate::mr_delete_edge("Uf6ce05bc4e5a","U499f24158a40","");
        let _ = crate::mr_delete_edge("U8a78048d60f7","Cbce32a9b256a","");
        let _ = crate::mr_delete_edge("U00ace0c36154","U000000000000","");
        let _ = crate::mr_delete_edge("U9a89e0679dec","Bf3a0a1165271","");
        let _ = crate::mr_delete_edge("U01814d1ec9ff","U000000000000","");
        let _ = crate::mr_delete_edge("Ub1a7f706910f","U000000000000","");
        let _ = crate::mr_delete_edge("U01814d1ec9ff","B63fbe1427d09","");
        let _ = crate::mr_delete_edge("U03eaee0e3052","U000000000000","");
        let _ = crate::mr_delete_edge("U0cd6bd2dde4f","Bc4addf09b79f","");
        let _ = crate::mr_delete_edge("U1bcba4fd7175","B4f00e7813add","");
        let _ = crate::mr_delete_edge("Ub01f4ad1b03f","U000000000000","");
        let _ = crate::mr_delete_edge("U996b5f6b8bec","U000000000000","");
        let _ = crate::mr_delete_edge("U9e42f6dab85a","Bad1c69de7837","");
        let _ = crate::mr_delete_edge("U26aca0e369c7","C599f6e6f6b64","");
        let _ = crate::mr_delete_edge("U09cf1f359454","Bdf39d0e1daf5","");
        let _ = crate::mr_delete_edge("Uf8bf10852d43","B253177f84f08","");
        let _ = crate::mr_delete_edge("U7a8d8324441d","B7f628ad203b5","");
        let _ = crate::mr_delete_edge("U43dcf522b4dd","B9c01ce5718d1","");
        let _ = crate::mr_delete_edge("C13e2a35d917a","Uf6ce05bc4e5a","");
        let _ = crate::mr_delete_edge("Ub01f4ad1b03f","B8a531802473b","");
        let _ = crate::mr_delete_edge("Uf5ee43a1b729","U000000000000","");
        let _ = crate::mr_delete_edge("U499f24158a40","C247501543b60","");
        let _ = crate::mr_delete_edge("C2e31b4b1658f","U8a78048d60f7","");
        let _ = crate::mr_delete_edge("U3c63a9b6115a","U000000000000","");
        let _ = crate::mr_delete_edge("C94bb73c10a06","Uef7fbf45ef11","");
        let _ = crate::mr_delete_edge("C357396896bd0","Udece0afd9a8b","");
        let _ = crate::mr_delete_edge("C6acd550a4ef3","Uc1158424318a","");
        let _ = crate::mr_delete_edge("U016217c34c6e","C3e84102071d1","");
        let _ = crate::mr_delete_edge("U18a178de1dfb","Bc4addf09b79f","");
        let _ = crate::mr_delete_edge("U499f24158a40","Cfe90cbd73eab","");
        let _ = crate::mr_delete_edge("U80e22da6d8c4","C30e7409c2d5f","");
        let _ = crate::mr_delete_edge("Uc8bb404462a4","U000000000000","");
        let _ = crate::mr_delete_edge("U09cf1f359454","Bf3a0a1165271","");
        let _ = crate::mr_delete_edge("U14a3c81256ab","U000000000000","");
        let _ = crate::mr_delete_edge("Uadeb43da4abb","C2bbd63b00224","");
        let _ = crate::mr_delete_edge("Ub01f4ad1b03f","Be2b46c17f1da","");
        let _ = crate::mr_delete_edge("U3f840973f9b5","U000000000000","");
        let _ = crate::mr_delete_edge("Caa62fc21e191","U4ba2e4e81c0e","");
        let _ = crate::mr_delete_edge("U02fbd7c8df4c","Bad1c69de7837","");
        let _ = crate::mr_delete_edge("U1bcba4fd7175","B73a44e2bbd44","");
        let _ = crate::mr_delete_edge("U37f5b0f1e914","U000000000000","");
        let _ = crate::mr_delete_edge("U80e22da6d8c4","U9e42f6dab85a","");
        let _ = crate::mr_delete_edge("Cb95e21215efa","U499f24158a40","");
        let _ = crate::mr_delete_edge("U35108003593e","U000000000000","");
        let _ = crate::mr_delete_edge("B1533941e2773","U79466f73dc0c","");
        let _ = crate::mr_delete_edge("U1e41b5f3adff","Ba5d64165e5d5","");
        let _ = crate::mr_delete_edge("U118afa836f11","U000000000000","");
        let _ = crate::mr_delete_edge("U682c3380036f","Bf34ee3bfc12b","");
        let _ = crate::mr_delete_edge("Udece0afd9a8b","Uc3c31b8a022f","");
        let _ = crate::mr_delete_edge("U6d2f25cc4264","U1c285703fc63","");
        let _ = crate::mr_delete_edge("U7a8d8324441d","U000000000000","");
        let _ = crate::mr_delete_edge("U0d47e4861ef0","U000000000000","");
        let _ = crate::mr_delete_edge("U1779c42930af","U000000000000","");
        let _ = crate::mr_delete_edge("Uc67c60f504ce","U000000000000","");
        let _ = crate::mr_delete_edge("U36ddff1a63d8","U000000000000","");
        let _ = crate::mr_delete_edge("U6661263fb410","U000000000000","");
        let _ = crate::mr_delete_edge("U9ce5721e93cf","B68247950d9c0","");
        let _ = crate::mr_delete_edge("Uf8bf10852d43","U000000000000","");
        let _ = crate::mr_delete_edge("U8456b2b56820","U000000000000","");
        let _ = crate::mr_delete_edge("U389f9f24b31c","Uc3c31b8a022f","");
        let _ = crate::mr_delete_edge("U9a89e0679dec","Cd06fea6a395f","");
        let _ = crate::mr_delete_edge("U9e42f6dab85a","C6a2263dc469e","");
        let _ = crate::mr_delete_edge("Ub01f4ad1b03f","B0a87a669fc28","");
        let _ = crate::mr_delete_edge("Cf40e8fb326bc","U526f361717a8","");
        let _ = crate::mr_delete_edge("U4ba2e4e81c0e","Cb117f464e558","");
        let _ = crate::mr_delete_edge("U47b466d57da1","U000000000000","");
        let _ = crate::mr_delete_edge("U526f361717a8","U000000000000","");
        let _ = crate::mr_delete_edge("Ub93799d9400e","U000000000000","");
        let _ = crate::mr_delete_edge("C524134905072","Ucb84c094edba","");
        let _ = crate::mr_delete_edge("Cd59e6cd7e104","U80e22da6d8c4","");
        let _ = crate::mr_delete_edge("Uaa4e2be7a87a","U000000000000","");
        let _ = crate::mr_delete_edge("Uc3a2aab8a776","U000000000000","");
        let _ = crate::mr_delete_edge("U9a89e0679dec","Cbbf2df46955b","");
        let _ = crate::mr_delete_edge("U38fdca6685ca","U000000000000","");
        let _ = crate::mr_delete_edge("Cbe89905f07d3","Ub01f4ad1b03f","");
        let _ = crate::mr_delete_edge("Bed5126bc655d","Uc4ebbce44401","");
        let _ = crate::mr_delete_edge("U9605bd4d1218","B8a531802473b","");
        let _ = crate::mr_delete_edge("Ueb139752b907","U000000000000","");
        let _ = crate::mr_delete_edge("Ub93799d9400e","B73a44e2bbd44","");
        let _ = crate::mr_delete_edge("Cee9901f0f22c","U526f361717a8","");
        let _ = crate::mr_delete_edge("Ub01f4ad1b03f","Cc2b3069cbe5d","");
        let _ = crate::mr_delete_edge("U7a8d8324441d","B3b3f2ecde430","");
        let _ = crate::mr_delete_edge("Ubebfe0c8fc29","Cfa08a39f9bb9","");
        let _ = crate::mr_delete_edge("C6587e913fbbe","U6661263fb410","");
        let _ = crate::mr_delete_edge("U895fd30e1e2a","U000000000000","");
        let _ = crate::mr_delete_edge("U0cd6bd2dde4f","C5782d559baad","");
        let _ = crate::mr_delete_edge("U6d2f25cc4264","B25c85fe0df2d","");
        let _ = crate::mr_delete_edge("U1bcba4fd7175","U000000000000","");
        let _ = crate::mr_delete_edge("U016217c34c6e","B8a531802473b","");
        let _ = crate::mr_delete_edge("Ccc25a77bfa2a","U77f496546efa","");
        let _ = crate::mr_delete_edge("U6240251593cd","Bf34ee3bfc12b","");
        let _ = crate::mr_delete_edge("U8a78048d60f7","C357396896bd0","");
        let _ = crate::mr_delete_edge("Uf2b0a6b1d423","Cb76829a425d9","");
        let _ = crate::mr_delete_edge("Ue7a29d5409f2","U016217c34c6e","");
        let _ = crate::mr_delete_edge("Ucdffb8ab5145","B9c01ce5718d1","");
        let _ = crate::mr_delete_edge("U01814d1ec9ff","B75a44a52fa29","");
        let _ = crate::mr_delete_edge("Cac6ca02355da","U6d2f25cc4264","");
        let _ = crate::mr_delete_edge("Ub01f4ad1b03f","Bc4addf09b79f","");
        let _ = crate::mr_delete_edge("U831a82104a9e","U000000000000","");
        let _ = crate::mr_delete_edge("U0c17798eaab4","U389f9f24b31c","");
        let _ = crate::mr_delete_edge("U0453a921d0e7","U000000000000","");
        let _ = crate::mr_delete_edge("U8a78048d60f7","Ud9df8116deba","");
        let _ = crate::mr_delete_edge("Ucd424ac24c15","B9c01ce5718d1","");
        let _ = crate::mr_delete_edge("U1bcba4fd7175","B9c01ce5718d1","");
        let _ = crate::mr_delete_edge("Ua7759a06a90a","U000000000000","");
        let _ = crate::mr_delete_edge("U3c63a9b6115a","B75a44a52fa29","");
        let _ = crate::mr_delete_edge("Bea16f01b8cc5","U1df3e39ebe59","");
        let _ = crate::mr_delete_edge("U1eafbaaf9536","U000000000000","");
        let _ = crate::mr_delete_edge("Uef7fbf45ef11","C3fd1fdebe0e9","");
        let _ = crate::mr_delete_edge("U73057a8e8ebf","U000000000000","");
        let _ = crate::mr_delete_edge("Cffd169930956","U0e6659929c53","");
        let _ = crate::mr_delete_edge("U01814d1ec9ff","B3b3f2ecde430","");
        let _ = crate::mr_delete_edge("Uf2b0a6b1d423","C4e0db8dec53e","");
        let _ = crate::mr_delete_edge("Ua9f1d3f8ee78","U000000000000","");
        let _ = crate::mr_delete_edge("U83282a51b600","B9c01ce5718d1","");
        let _ = crate::mr_delete_edge("Uf5096f6ab14e","U9e42f6dab85a","");
        let _ = crate::mr_delete_edge("U6d2f25cc4264","Bfefe4e25c870","");
        let _ = crate::mr_delete_edge("U80e22da6d8c4","C070e739180d6","");
        let _ = crate::mr_delete_edge("C8343a6a576ff","U02fbd7c8df4c","");
        let _ = crate::mr_delete_edge("Udece0afd9a8b","C599f6e6f6b64","");
        let _ = crate::mr_delete_edge("U77f496546efa","C9462ca240ceb","");
        let _ = crate::mr_delete_edge("Cc42c3eeb9d20","U8a78048d60f7","");
        let _ = crate::mr_delete_edge("Uf2b0a6b1d423","Ce1a7d8996eb0","");
        let _ = crate::mr_delete_edge("U1bcba4fd7175","B70df5dbab8c3","");
        let _ = crate::mr_delete_edge("Ub10b78df4f63","U000000000000","");
        let _ = crate::mr_delete_edge("Uaa4e2be7a87a","C35678a54ef5f","");
        let _ = crate::mr_delete_edge("U59abf06369c3","U000000000000","");
        let _ = crate::mr_delete_edge("U3de789cac826","U000000000000","");
        let _ = crate::mr_delete_edge("U72f88cf28226","C7722465c957a","");
        let _ = crate::mr_delete_edge("U9605bd4d1218","C801f204d0da8","");
        let _ = crate::mr_delete_edge("U8a78048d60f7","B10d3f548efc4","");
        let _ = crate::mr_delete_edge("U02be55e5fdb2","U000000000000","");
        let _ = crate::mr_delete_edge("U9a89e0679dec","U7a8d8324441d","");
        let _ = crate::mr_delete_edge("Be7145faf15cb","Ud982a6dee46f","");
        let _ = crate::mr_delete_edge("U7f5fca21e1e5","U000000000000","");
        let _ = crate::mr_delete_edge("Cd06fea6a395f","Uaa4e2be7a87a","");
        let _ = crate::mr_delete_edge("U7a8d8324441d","C78ad459d3b81","");
        let _ = crate::mr_delete_edge("Udf0362755172","U000000000000","");
        let _ = crate::mr_delete_edge("U28f934dc948e","U000000000000","");
        let _ = crate::mr_delete_edge("Bb1e3630d2f4a","U34252014c05b","");
        let _ = crate::mr_delete_edge("U6661263fb410","B75a44a52fa29","");
        let _ = crate::mr_delete_edge("U495c3bb411e1","U000000000000","");
        let _ = crate::mr_delete_edge("Uadeb43da4abb","Bd49e3dac97b0","");
        let _ = crate::mr_delete_edge("Ub93799d9400e","Cd4417a5d718e","");
        let _ = crate::mr_delete_edge("C399b6349ab02","Uf2b0a6b1d423","");
        let _ = crate::mr_delete_edge("Ue73fabd3d39a","U000000000000","");
        let _ = crate::mr_delete_edge("U4dac6797a9cc","U000000000000","");
        let _ = crate::mr_delete_edge("Ua29a81d30ef9","U000000000000","");
        let _ = crate::mr_delete_edge("Ud5b22ebf52f2","U000000000000","");
        let _ = crate::mr_delete_edge("B79efabc4d8bf","U499f24158a40","");
        let _ = crate::mr_delete_edge("U83282a51b600","C16dfdd8077c8","");
        let _ = crate::mr_delete_edge("U1c285703fc63","U9a2c85753a6d","");
        let _ = crate::mr_delete_edge("B19ea554faf29","U34252014c05b","");
        let _ = crate::mr_delete_edge("B75a44a52fa29","U01814d1ec9ff","");
        let _ = crate::mr_delete_edge("C35678a54ef5f","Uaa4e2be7a87a","");
        let _ = crate::mr_delete_edge("Uc3c31b8a022f","Bb78026d99388","");
        let _ = crate::mr_delete_edge("U09cf1f359454","Be5bb2f3d56cb","");
        let _ = crate::mr_delete_edge("U4dd243415525","U000000000000","");
        let _ = crate::mr_delete_edge("Cb62aea64ea97","U0e6659929c53","");
        let _ = crate::mr_delete_edge("Uef7fbf45ef11","B25c85fe0df2d","");
        let _ = crate::mr_delete_edge("Uefe16d246c36","U000000000000","");
        let _ = crate::mr_delete_edge("U09cf1f359454","B8a531802473b","");
        let _ = crate::mr_delete_edge("C5127d08eb786","Ucd424ac24c15","");
        let _ = crate::mr_delete_edge("U7a8d8324441d","Be2b46c17f1da","");
        let _ = crate::mr_delete_edge("U8a78048d60f7","U1c285703fc63","");
        let _ = crate::mr_delete_edge("Uf2b0a6b1d423","C6a2263dc469e","");
        let _ = crate::mr_delete_edge("Uef7fbf45ef11","B0e230e9108dd","");
        let _ = crate::mr_delete_edge("U53eb1f0bdcd2","U000000000000","");
        let _ = crate::mr_delete_edge("U8a78048d60f7","Uad577360d968","");
        let _ = crate::mr_delete_edge("U1afee48387d4","U000000000000","");
        let _ = crate::mr_delete_edge("U09cf1f359454","B4f14b223b56d","");
        let _ = crate::mr_delete_edge("B3c467fb437b2","U9e42f6dab85a","");
        let _ = crate::mr_delete_edge("U9a2c85753a6d","C30fef1977b4a","");
        let _ = crate::mr_delete_edge("U8a78048d60f7","B19ea554faf29","");
        let _ = crate::mr_delete_edge("U6240251593cd","B9c01ce5718d1","");
        let _ = crate::mr_delete_edge("U99a0f1f7e6ee","C1f41b842849c","");
        let _ = crate::mr_delete_edge("Uac897fe92894","Cb117f464e558","");
        let _ = crate::mr_delete_edge("U704bd6ecde75","U000000000000","");
        let _ = crate::mr_delete_edge("U09cf1f359454","U0cd6bd2dde4f","");
        let _ = crate::mr_delete_edge("Ucb84c094edba","C524134905072","");
        let _ = crate::mr_delete_edge("B19d70698e3d8","Uf8bf10852d43","");
        let _ = crate::mr_delete_edge("Cda989f4b466d","U59abf06369c3","");
        let _ = crate::mr_delete_edge("Ub01f4ad1b03f","B1533941e2773","");
        let _ = crate::mr_delete_edge("U83e829a2e822","B5eb4c6be535a","");
        let _ = crate::mr_delete_edge("U34252014c05b","U000000000000","");
        let _ = crate::mr_delete_edge("Ud9df8116deba","U000000000000","");
        let _ = crate::mr_delete_edge("U01814d1ec9ff","Bd7a8bfcf3337","");
        let _ = crate::mr_delete_edge("U918f8950c4e5","U000000000000","");
        let _ = crate::mr_delete_edge("Cfdde53c79a2d","Uef7fbf45ef11","");
        let _ = crate::mr_delete_edge("Ue6cc7bfa0efd","B30bf91bf5845","");
        let _ = crate::mr_delete_edge("U09cf1f359454","B3b3f2ecde430","");
        let _ = crate::mr_delete_edge("C63e21d051dda","U638f5c19326f","");
        let _ = crate::mr_delete_edge("Uf2b0a6b1d423","C30e7409c2d5f","");
        let _ = crate::mr_delete_edge("Ue7a29d5409f2","C9028c7415403","");
        let _ = crate::mr_delete_edge("U09cf1f359454","Bd49e3dac97b0","");
        let _ = crate::mr_delete_edge("Ua34e02cf30a6","U000000000000","");
        let _ = crate::mr_delete_edge("C279db553a831","U99a0f1f7e6ee","");
        let _ = crate::mr_delete_edge("Ub01f4ad1b03f","B45d72e29f004","");
        let _ = crate::mr_delete_edge("U01814d1ec9ff","B491d307dfe01","");
        let _ = crate::mr_delete_edge("U99a0f1f7e6ee","Cfd59a206c07d","");
        let _ = crate::mr_delete_edge("C52d41a9ad558","U526f361717a8","");
        let _ = crate::mr_delete_edge("U7462db3b65c4","U000000000000","");
        let _ = crate::mr_delete_edge("U79466f73dc0c","U000000000000","");
        let _ = crate::mr_delete_edge("Ue7a29d5409f2","Uc3c31b8a022f","");
        let _ = crate::mr_delete_edge("Uf9ecad50b7e1","U000000000000","");
        let _ = crate::mr_delete_edge("U1bcba4fd7175","C0a576fc389d9","");
        let _ = crate::mr_delete_edge("Uef7fbf45ef11","C94bb73c10a06","");
        let _ = crate::mr_delete_edge("U3614888a1bdc","U000000000000","");
        let _ = crate::mr_delete_edge("U8a78048d60f7","Ud5b22ebf52f2","");
        let _ = crate::mr_delete_edge("Uf8bf10852d43","B4115d364e05b","");
        let _ = crate::mr_delete_edge("U57b6f30fc663","B30bf91bf5845","");
        let _ = crate::mr_delete_edge("U72f88cf28226","U6d2f25cc4264","");
        let _ = crate::mr_delete_edge("U3c63a9b6115a","Be5bb2f3d56cb","");
        let _ = crate::mr_delete_edge("C7722465c957a","U72f88cf28226","");
        let _ = crate::mr_delete_edge("Ub7f9dfb6a7a5","B506fff6cfc22","");
        let _ = crate::mr_delete_edge("Udece0afd9a8b","C9028c7415403","");
        let _ = crate::mr_delete_edge("U79466f73dc0c","B45d72e29f004","");
        let _ = crate::mr_delete_edge("U67bf00435429","U000000000000","");
        let _ = crate::mr_delete_edge("U3bbfefd5319e","U000000000000","");
        let _ = crate::mr_delete_edge("U99a0f1f7e6ee","U000000000000","");
        let _ = crate::mr_delete_edge("U09cf1f359454","B3f6f837bc345","");
        let _ = crate::mr_delete_edge("U8a78048d60f7","B4f14b223b56d","");
        let _ = crate::mr_delete_edge("U0e6659929c53","U000000000000","");
        let _ = crate::mr_delete_edge("U6661263fb410","C31dac67e313b","");
        let _ = crate::mr_delete_edge("C55a114ca6e7c","U0e6659929c53","");
        let _ = crate::mr_delete_edge("Ue328d7da3b59","U000000000000","");
        let _ = crate::mr_delete_edge("C4b2b6fd8fa9a","U499f24158a40","");
        let _ = crate::mr_delete_edge("U9e42f6dab85a","C0b19d314485e","");
        let _ = crate::mr_delete_edge("U6d2f25cc4264","Ba3c4a280657d","");
        let _ = crate::mr_delete_edge("Ub01f4ad1b03f","B70df5dbab8c3","");
        let _ = crate::mr_delete_edge("Uf8eb8562f949","U000000000000","");
        let _ = crate::mr_delete_edge("U7a8d8324441d","C30fef1977b4a","");
        let _ = crate::mr_delete_edge("Uc35c445325f5","Be29b4af3f7a5","");
        let _ = crate::mr_delete_edge("Uebe87839ab3e","U000000000000","");
        let _ = crate::mr_delete_edge("Ud826f91f9025","U000000000000","");
        let _ = crate::mr_delete_edge("U7a8d8324441d","B5eb4c6be535a","");
        let _ = crate::mr_delete_edge("Uf2b0a6b1d423","Cdcddfb230cb5","");
        let _ = crate::mr_delete_edge("U9605bd4d1218","Bd7a8bfcf3337","");
        let _ = crate::mr_delete_edge("U4e7d43caba8f","U000000000000","");
        let _ = crate::mr_delete_edge("U499f24158a40","B9c01ce5718d1","");
        let _ = crate::mr_delete_edge("U7a8d8324441d","B3b3f2ecde430","");
        let _ = crate::mr_delete_edge("U83282a51b600","B45d72e29f004","");
        let _ = crate::mr_delete_edge("U4db49066d45a","U000000000000","");
        let _ = crate::mr_delete_edge("Ub01f4ad1b03f","B60d725feca77","");
        let _ = crate::mr_delete_edge("U21769235b28d","U000000000000","");
        let _ = crate::mr_delete_edge("U80e22da6d8c4","Cd59e6cd7e104","");
        let _ = crate::mr_delete_edge("U26aca0e369c7","C9028c7415403","");
        let _ = crate::mr_delete_edge("U161742354fef","U000000000000","");
        let _ = crate::mr_delete_edge("Udece0afd9a8b","U000000000000","");
        let _ = crate::mr_delete_edge("B9cade9992fb9","U638f5c19326f","");
        let _ = crate::mr_delete_edge("U3b6ea55b4098","U000000000000","");
        let _ = crate::mr_delete_edge("U05e4396e2382","U000000000000","");
        let _ = crate::mr_delete_edge("U09cf1f359454","B0e230e9108dd","");
        let _ = crate::mr_delete_edge("U499f24158a40","U6d2f25cc4264","");
        let _ = crate::mr_delete_edge("U79466f73dc0c","Be2b46c17f1da","");
        let _ = crate::mr_delete_edge("U27847df66cb4","U000000000000","");
        let _ = crate::mr_delete_edge("C9218f86f6286","Uf5ee43a1b729","");
        let _ = crate::mr_delete_edge("U6d2f25cc4264","Bb78026d99388","");
        let _ = crate::mr_delete_edge("U9a2c85753a6d","B3b3f2ecde430","");
        let _ = crate::mr_delete_edge("U8a78048d60f7","Bd49e3dac97b0","");
        let _ = crate::mr_delete_edge("Uf5096f6ab14e","C9462ca240ceb","");
        let _ = crate::mr_delete_edge("U1bcba4fd7175","B0e230e9108dd","");
        let _ = crate::mr_delete_edge("U9a2c85753a6d","Uf5096f6ab14e","");
        let _ = crate::mr_delete_edge("U0a5d1c56f5a1","U000000000000","");
        let _ = crate::mr_delete_edge("Ue7a29d5409f2","Uf2b0a6b1d423","");
        let _ = crate::mr_delete_edge("Ub01f4ad1b03f","Bd90a1cf73384","");
        let _ = crate::mr_delete_edge("Ucb84c094edba","C2cb023b6bcef","");
        let _ = crate::mr_delete_edge("Udfbfcd087e6b","U000000000000","");
        let _ = crate::mr_delete_edge("U3bf4a5894df1","U000000000000","");
        let _ = crate::mr_delete_edge("Ubebfe0c8fc29","Bfefe4e25c870","");
        let _ = crate::mr_delete_edge("U9e42f6dab85a","C070e739180d6","");
        let _ = crate::mr_delete_edge("U6d2f25cc4264","C6f84810d3cd9","");
        let _ = crate::mr_delete_edge("U14a3c81256ab","B9c01ce5718d1","");
        let _ = crate::mr_delete_edge("Cd5983133fb67","U8a78048d60f7","");
        let _ = crate::mr_delete_edge("U675d1026fe95","U000000000000","");
        let _ = crate::mr_delete_edge("U0cd6bd2dde4f","B75a44a52fa29","");
        let _ = crate::mr_delete_edge("Ue7a29d5409f2","Ce1a7d8996eb0","");
        let _ = crate::mr_delete_edge("Ue40b938f47a4","B9c01ce5718d1","");
        let _ = crate::mr_delete_edge("U0e6659929c53","Cb62aea64ea97","");
        let _ = crate::mr_delete_edge("U732b06e17fc6","U000000000000","");
        let _ = crate::mr_delete_edge("C1c86825bd597","U01814d1ec9ff","");
        let _ = crate::mr_delete_edge("U09cf1f359454","Bb78026d99388","");
        let _ = crate::mr_delete_edge("U393de9ce9ec4","U000000000000","");
        let _ = crate::mr_delete_edge("U389f9f24b31c","Cbce32a9b256a","");
        let _ = crate::mr_delete_edge("U499f24158a40","C96bdee4f11e2","");
        let _ = crate::mr_delete_edge("Uc76658319bfe","U000000000000","");
        let _ = crate::mr_delete_edge("U389f9f24b31c","C4893c40e481d","");
        let _ = crate::mr_delete_edge("Uef7fbf45ef11","B7f628ad203b5","");
        let _ = crate::mr_delete_edge("Uad577360d968","Bad1c69de7837","");
        let _ = crate::mr_delete_edge("U65bb6831c537","U000000000000","");
        let _ = crate::mr_delete_edge("U3b78f50182c7","U000000000000","");
        let _ = crate::mr_delete_edge("Ud982a6dee46f","U000000000000","");
        let _ = crate::mr_delete_edge("U499f24158a40","Cdeab5b39cc2a","");
        let _ = crate::mr_delete_edge("B7f628ad203b5","U7a8d8324441d","");
        let _ = crate::mr_delete_edge("Udece0afd9a8b","C4893c40e481d","");
        let _ = crate::mr_delete_edge("C96bdee4f11e2","U499f24158a40","");
        let _ = crate::mr_delete_edge("U0b4010c6af8e","U000000000000","");
        let _ = crate::mr_delete_edge("Ub01f4ad1b03f","B0e230e9108dd","");
        let _ = crate::mr_delete_edge("U02fbd7c8df4c","C8343a6a576ff","");
        let _ = crate::mr_delete_edge("C30fef1977b4a","U7a8d8324441d","");
        let _ = crate::mr_delete_edge("U77f496546efa","Ccc25a77bfa2a","");
        let _ = crate::mr_delete_edge("Uf6ce05bc4e5a","C13e2a35d917a","");
        let _ = crate::mr_delete_edge("Ucbb6d026b66f","U000000000000","");
        let _ = crate::mr_delete_edge("U6d2f25cc4264","Cfe90cbd73eab","");
        let _ = crate::mr_delete_edge("U0c17798eaab4","B3c467fb437b2","");
        let _ = crate::mr_delete_edge("Ue6cc7bfa0efd","U000000000000","");
        let _ = crate::mr_delete_edge("Ue4f003e63773","U000000000000","");
        let _ = crate::mr_delete_edge("U9e42f6dab85a","U80e22da6d8c4","");
        let _ = crate::mr_delete_edge("U7cdd7999301e","U000000000000","");
        let _ = crate::mr_delete_edge("Uef7fbf45ef11","Cfdde53c79a2d","");
        let _ = crate::mr_delete_edge("U7a8d8324441d","C94bb73c10a06","");
        let _ = crate::mr_delete_edge("U8a78048d60f7","Bb78026d99388","");
        let _ = crate::mr_delete_edge("U6d2f25cc4264","B499bfc56e77b","");
        let _ = crate::mr_delete_edge("U0c17798eaab4","Ce1a7d8996eb0","");
        let _ = crate::mr_delete_edge("C7062e90f7422","U01814d1ec9ff","");
        let _ = crate::mr_delete_edge("Cf8fb8c05c116","Ucdffb8ab5145","");
        let _ = crate::mr_delete_edge("B60d725feca77","U80e22da6d8c4","");
        let _ = crate::mr_delete_edge("C070e739180d6","U80e22da6d8c4","");
        let _ = crate::mr_delete_edge("C3e84102071d1","U016217c34c6e","");
        let _ = crate::mr_delete_edge("B69723edfec8a","U5c827d7de115","");
        let _ = crate::mr_delete_edge("U0c17798eaab4","B0e230e9108dd","");
        let _ = crate::mr_delete_edge("U0c17798eaab4","C4e0db8dec53e","");
        let _ = crate::mr_delete_edge("Uf2b0a6b1d423","U000000000000","");
        let _ = crate::mr_delete_edge("U389f9f24b31c","Cfc639b9aa3e0","");
        let _ = crate::mr_delete_edge("Uad577360d968","C0cd490b5fb6a","");
        let _ = crate::mr_delete_edge("Ucc8ea98c2b41","U000000000000","");
        let _ = crate::mr_delete_edge("Uc3c31b8a022f","U1c285703fc63","");
        let _ = crate::mr_delete_edge("U3c63a9b6115a","B9c01ce5718d1","");
        let _ = crate::mr_delete_edge("B3b3f2ecde430","U7a8d8324441d","");
        let _ = crate::mr_delete_edge("U38fdca6685ca","B9c01ce5718d1","");
        let _ = crate::mr_delete_edge("Ua1ca6a97ea28","U000000000000","");
        let _ = crate::mr_delete_edge("U9a2c85753a6d","C78ad459d3b81","");
        let _ = crate::mr_delete_edge("Uc1158424318a","C67e4476fda28","");
        let _ = crate::mr_delete_edge("Ub01f4ad1b03f","U01814d1ec9ff","");
        let _ = crate::mr_delete_edge("Ua5a9eab9732d","U000000000000","");
        let _ = crate::mr_delete_edge("U4ba2e4e81c0e","Ca8ceac412e6f","");
        let _ = crate::mr_delete_edge("Cfe90cbd73eab","U499f24158a40","");
        let _ = crate::mr_delete_edge("U79466f73dc0c","B1533941e2773","");
        let _ = crate::mr_delete_edge("C8d80016b8292","U499f24158a40","");
        let _ = crate::mr_delete_edge("Uf6ce05bc4e5a","Bf843e315d71b","");
        let _ = crate::mr_delete_edge("U6661263fb410","C6587e913fbbe","");
        let _ = crate::mr_delete_edge("U8a78048d60f7","Bb1e3630d2f4a","");
        let _ = crate::mr_delete_edge("Ucd424ac24c15","C5127d08eb786","");
        let _ = crate::mr_delete_edge("Uc1158424318a","C4e0db8dec53e","");
        let _ = crate::mr_delete_edge("U6d2f25cc4264","Bad1c69de7837","");
        let _ = crate::mr_delete_edge("Ud7002ae5a86c","C7a807e462b65","");
        let _ = crate::mr_delete_edge("C3c17b70c3357","U3de789cac826","");
        let _ = crate::mr_delete_edge("U5cfee124371b","U000000000000","");
        let _ = crate::mr_delete_edge("Bd90a1cf73384","U99a0f1f7e6ee","");
        let _ = crate::mr_delete_edge("Uc3a349f521e1","U000000000000","");
        let _ = crate::mr_delete_edge("U83282a51b600","B7f628ad203b5","");
        let _ = crate::mr_delete_edge("U0cd6bd2dde4f","U000000000000","");
        let _ = crate::mr_delete_edge("U26aca0e369c7","Cb117f464e558","");
        let _ = crate::mr_delete_edge("Ce1a7d8996eb0","Uf5096f6ab14e","");
        let _ = crate::mr_delete_edge("U09cf1f359454","Bad1c69de7837","");
        let _ = crate::mr_delete_edge("Ub01f4ad1b03f","B25c85fe0df2d","");
        let _ = crate::mr_delete_edge("U9a89e0679dec","C6aebafa4fe8e","");
        let _ = crate::mr_delete_edge("U01814d1ec9ff","B8a531802473b","");
        let _ = crate::mr_delete_edge("Ub93799d9400e","B75a44a52fa29","");
        let _ = crate::mr_delete_edge("Ub01f4ad1b03f","Bf34ee3bfc12b","");
        let _ = crate::mr_delete_edge("U34252014c05b","B0a87a669fc28","");
        let _ = crate::mr_delete_edge("Ud18285ef1202","U000000000000","");
        let _ = crate::mr_delete_edge("Ub01f4ad1b03f","Cbe89905f07d3","");
        let _ = crate::mr_delete_edge("U7c88b933c58d","U000000000000","");
        let _ = crate::mr_delete_edge("U4ba2e4e81c0e","U000000000000","");
        let _ = crate::mr_delete_edge("U8889e390d38b","U000000000000","");
        let _ = crate::mr_delete_edge("U9e972ae23870","U000000000000","");
        let _ = crate::mr_delete_edge("U389f9f24b31c","U7a8d8324441d","");
        let _ = crate::mr_delete_edge("Uf5096f6ab14e","Ce1a7d8996eb0","");
        let _ = crate::mr_delete_edge("U09cf1f359454","B4f00e7813add","");
        let _ = crate::mr_delete_edge("Bd7a8bfcf3337","U02fbd7c8df4c","");
        let _ = crate::mr_delete_edge("C2d9ab331aed7","U4a82930ca419","");
        let _ = crate::mr_delete_edge("C247501543b60","U499f24158a40","");
        let _ = crate::mr_delete_edge("U9a2c85753a6d","Cfdde53c79a2d","");
        let _ = crate::mr_delete_edge("U80e22da6d8c4","C613f00c1333c","");
        let _ = crate::mr_delete_edge("C31dac67e313b","U6661263fb410","");
        let _ = crate::mr_delete_edge("U864ef33f7249","U000000000000","");
        let _ = crate::mr_delete_edge("Uf91b831f1eb7","U000000000000","");
        let _ = crate::mr_delete_edge("Ua85bc934db95","U000000000000","");
        let _ = crate::mr_delete_edge("C67e4476fda28","U1c285703fc63","");
        let _ = crate::mr_delete_edge("U09cf1f359454","U000000000000","");
        let _ = crate::mr_delete_edge("U80e22da6d8c4","C3e84102071d1","");
        let _ = crate::mr_delete_edge("U9605bd4d1218","Cab47a458295f","");
        let _ = crate::mr_delete_edge("U6d2f25cc4264","C992d8370db6b","");
        let _ = crate::mr_delete_edge("Cbce32a9b256a","U389f9f24b31c","");
        let _ = crate::mr_delete_edge("Uc1158424318a","U000000000000","");
        let _ = crate::mr_delete_edge("U09cf1f359454","B63fbe1427d09","");
        let _ = crate::mr_delete_edge("B63fbe1427d09","U1c285703fc63","");
        let _ = crate::mr_delete_edge("Uf5096f6ab14e","B60d725feca77","");
        let _ = crate::mr_delete_edge("U1bcba4fd7175","Bfefe4e25c870","");
        let _ = crate::mr_delete_edge("U09cf1f359454","B9c01ce5718d1","");
        let _ = crate::mr_delete_edge("U83282a51b600","U000000000000","");
        let _ = crate::mr_delete_edge("Uac897fe92894","B7f628ad203b5","");
        let _ = crate::mr_delete_edge("B8120aa1edccb","Ue40b938f47a4","");
        let _ = crate::mr_delete_edge("Ubbe66e390603","U000000000000","");
        let _ = crate::mr_delete_edge("Ueb139752b907","U79466f73dc0c","");
        let _ = crate::mr_delete_edge("U1c285703fc63","U000000000000","");
        let _ = crate::mr_delete_edge("U0e6659929c53","B9c01ce5718d1","");
        let _ = crate::mr_delete_edge("U09cf1f359454","B491d307dfe01","");
        let _ = crate::mr_delete_edge("U18a178de1dfb","B73a44e2bbd44","");
        let _ = crate::mr_delete_edge("U38fdca6685ca","C958e7588ae1c","");
        let _ = crate::mr_delete_edge("U72f88cf28226","U000000000000","");
        let _ = crate::mr_delete_edge("Ue70d59cc8e3f","U000000000000","");
        let _ = crate::mr_delete_edge("U35eb26fc07b4","B60d725feca77","");
        let _ = crate::mr_delete_edge("Cdeab5b39cc2a","U499f24158a40","");
        let _ = crate::mr_delete_edge("U0f63ee3db59b","Cbcf72c7e6061","");
        let _ = crate::mr_delete_edge("C4e0db8dec53e","U0c17798eaab4","");
        let _ = crate::mr_delete_edge("U8a78048d60f7","B7f628ad203b5","");
        let _ = crate::mr_delete_edge("Uc78a29f47b21","U000000000000","");
        let _ = crate::mr_delete_edge("C472b59eeafa5","U4a82930ca419","");
        let _ = crate::mr_delete_edge("U6d2f25cc4264","C247501543b60","");
        let _ = crate::mr_delete_edge("Cfc639b9aa3e0","U389f9f24b31c","");
        let _ = crate::mr_delete_edge("U99deecf5a281","B9c01ce5718d1","");
        let _ = crate::mr_delete_edge("Bfefe4e25c870","U499f24158a40","");
        let _ = crate::mr_delete_edge("Ua12e78308f49","B75a44a52fa29","");
        let _ = crate::mr_delete_edge("U1d5b8c2a3400","U000000000000","");
        let _ = crate::mr_delete_edge("Ucdffb8ab5145","U000000000000","");
        let _ = crate::mr_delete_edge("Uaa4e2be7a87a","Cd06fea6a395f","");
        let _ = crate::mr_delete_edge("U682c3380036f","B75a44a52fa29","");
        let _ = crate::mr_delete_edge("Ue202d5b01f8d","C637133747308","");
        let _ = crate::mr_delete_edge("U6d2f25cc4264","Cab47a458295f","");
        let _ = crate::mr_delete_edge("U0c17798eaab4","Cd06fea6a395f","");
        let _ = crate::mr_delete_edge("Ub01f4ad1b03f","Bd7a8bfcf3337","");
        let _ = crate::mr_delete_edge("U8a78048d60f7","U01814d1ec9ff","");
        let _ = crate::mr_delete_edge("Ua01529fb0d57","U000000000000","");
        let _ = crate::mr_delete_edge("U611323f9392c","U000000000000","");
        let _ = crate::mr_delete_edge("Uf5ee43a1b729","B47cc49866c37","");
        let _ = crate::mr_delete_edge("Uac897fe92894","C9462ca240ceb","");
        let _ = crate::mr_delete_edge("U21769235b28d","C481cd737c873","");
        let _ = crate::mr_delete_edge("C6f84810d3cd9","U499f24158a40","");
        let _ = crate::mr_delete_edge("Ub93c197b25c5","U000000000000","");
        let _ = crate::mr_delete_edge("Uc3c31b8a022f","C78d6fac93d00","");
        let _ = crate::mr_delete_edge("Udece0afd9a8b","C4f2dafca724f","");
        let _ = crate::mr_delete_edge("U9361426a2e51","U000000000000","");
        let _ = crate::mr_delete_edge("Uaa4e2be7a87a","B0e230e9108dd","");
        let _ = crate::mr_delete_edge("Ub01f4ad1b03f","Bb78026d99388","");
        let _ = crate::mr_delete_edge("U8a78048d60f7","U000000000000","");
        let _ = crate::mr_delete_edge("U8a78048d60f7","Cdcddfb230cb5","");
        let _ = crate::mr_delete_edge("Ub01f4ad1b03f","U0cd6bd2dde4f","");
        let _ = crate::mr_delete_edge("Ud5b22ebf52f2","B310b66ab31fb","");
        let _ = crate::mr_delete_edge("U802de6b3675a","U000000000000","");
        let _ = crate::mr_delete_edge("U8a78048d60f7","B0e230e9108dd","");
        let _ = crate::mr_delete_edge("U09cf1f359454","U6d2f25cc4264","");
        let _ = crate::mr_delete_edge("Uad577360d968","Cbce32a9b256a","");
        let _ = crate::mr_delete_edge("U09cf1f359454","Be29b4af3f7a5","");
        let _ = crate::mr_delete_edge("U5e1dd853cab5","U000000000000","");
        let _ = crate::mr_delete_edge("C958e7588ae1c","U38fdca6685ca","");
        let _ = crate::mr_delete_edge("Ub152bb6d4a86","U000000000000","");
        let _ = crate::mr_delete_edge("U47b466d57da1","Bad1c69de7837","");
        let _ = crate::mr_delete_edge("U016217c34c6e","Ca0a6aea6c82e","");
        let _ = crate::mr_delete_edge("U18a178de1dfb","B491d307dfe01","");
        let _ = crate::mr_delete_edge("U79466f73dc0c","B9c01ce5718d1","");
        let _ = crate::mr_delete_edge("Ucfb9f0586d9e","U000000000000","");
        let _ = crate::mr_delete_edge("U704bd6ecde75","C3b855f713d19","");
        let _ = crate::mr_delete_edge("Uc1158424318a","C0b19d314485e","");
        let _ = crate::mr_delete_edge("U1c285703fc63","Uad577360d968","");
        let _ = crate::mr_delete_edge("Ce49159fe9d01","U6661263fb410","");
        let _ = crate::mr_delete_edge("U6d2f25cc4264","B8a531802473b","");
        let _ = crate::mr_delete_edge("U8a78048d60f7","B310b66ab31fb","");
        let _ = crate::mr_delete_edge("Ub22f9ca70b59","U000000000000","");
        let _ = crate::mr_delete_edge("U499f24158a40","B491d307dfe01","");
        let _ = crate::mr_delete_edge("Ue40b938f47a4","U000000000000","");
        let _ = crate::mr_delete_edge("Ud04c89aaf453","U8a78048d60f7","");
        let _ = crate::mr_delete_edge("U9605bd4d1218","B5a1c1d3d0140","");
        let _ = crate::mr_delete_edge("U2cb58c48703b","U000000000000","");
        let _ = crate::mr_delete_edge("Ud04c89aaf453","B73a44e2bbd44","");
        let _ = crate::mr_delete_edge("Ub01f4ad1b03f","B9c01ce5718d1","");
        let _ = crate::mr_delete_edge("Uc4ebbce44401","Bed5126bc655d","");
        let _ = crate::mr_delete_edge("U8a78048d60f7","B79efabc4d8bf","");
        let _ = crate::mr_delete_edge("Be5bb2f3d56cb","U3c63a9b6115a","");
        let _ = crate::mr_delete_edge("U8842ed397bb7","C89c123f7bcf5","");
        let _ = crate::mr_delete_edge("Uceaf0448e060","U000000000000","");
        let _ = crate::mr_delete_edge("Uc1158424318a","B7f628ad203b5","");
        let _ = crate::mr_delete_edge("U0f63ee3db59b","Cc616eded7a99","");
        let _ = crate::mr_delete_edge("U26aca0e369c7","B45d72e29f004","");
        let _ = crate::mr_delete_edge("Ubeded808a9c0","B9c01ce5718d1","");
        let _ = crate::mr_delete_edge("B30bf91bf5845","Ue6cc7bfa0efd","");
        let _ = crate::mr_delete_edge("U005d51b8771c","U000000000000","");
        let _ = crate::mr_delete_edge("Ubcf610883f95","U000000000000","");
        let _ = crate::mr_delete_edge("U499f24158a40","Cb95e21215efa","");
        let _ = crate::mr_delete_edge("U606a687682ec","U000000000000","");
        let _ = crate::mr_delete_edge("C16dfdd8077c8","U83282a51b600","");
        let _ = crate::mr_delete_edge("C1f41b842849c","U99a0f1f7e6ee","");
        let _ = crate::mr_delete_edge("Ue20d37fe1d62","U000000000000","");
        let _ = crate::mr_delete_edge("U1e5391821528","U000000000000","");
        let _ = crate::mr_delete_edge("U1c285703fc63","U6d2f25cc4264","");
        let _ = crate::mr_delete_edge("U09cf1f359454","B60d725feca77","");
        let _ = crate::mr_delete_edge("B3f6f837bc345","U6d2f25cc4264","");
        let _ = crate::mr_delete_edge("U704bd6ecde75","Cfd47f43ac9cf","");
        let _ = crate::mr_delete_edge("Ua12e78308f49","U000000000000","");
        let _ = crate::mr_delete_edge("U4a82930ca419","C472b59eeafa5","");
        let _ = crate::mr_delete_edge("U8a78048d60f7","Ub93799d9400e","");
        let _ = crate::mr_delete_edge("B47cc49866c37","Uf5ee43a1b729","");
        let _ = crate::mr_delete_edge("U6d2f25cc4264","B491d307dfe01","");
        let _ = crate::mr_delete_edge("U7a8d8324441d","C3fd1fdebe0e9","");
        let _ = crate::mr_delete_edge("U0c17798eaab4","U000000000000","");
        let _ = crate::mr_delete_edge("U0ff6902d8945","U000000000000","");
        let _ = crate::mr_delete_edge("U0e6659929c53","C55a114ca6e7c","");
        let _ = crate::mr_delete_edge("U7a8d8324441d","U6d2f25cc4264","");
        let _ = crate::mr_delete_edge("U638f5c19326f","C63e21d051dda","");
        let _ = crate::mr_delete_edge("U8ec514590d15","U000000000000","");
        let _ = crate::mr_delete_edge("U6240251593cd","B75a44a52fa29","");
        let _ = crate::mr_delete_edge("C7986cd8a648a","U682c3380036f","");
        let _ = crate::mr_delete_edge("C637133747308","Ue202d5b01f8d","");
        let _ = crate::mr_delete_edge("U9605bd4d1218","B9c01ce5718d1","");
        let _ = crate::mr_delete_edge("B68247950d9c0","U9ce5721e93cf","");
        let _ = crate::mr_delete_edge("Bf843e315d71b","Uf6ce05bc4e5a","");
        let _ = crate::mr_delete_edge("U5f8c0e9c8cc4","U000000000000","");
        let _ = crate::mr_delete_edge("U01814d1ec9ff","B5a1c1d3d0140","");
        let _ = crate::mr_delete_edge("U8a78048d60f7","B5eb4c6be535a","");
        let _ = crate::mr_delete_edge("Ubebfe0c8fc29","U000000000000","");
        let _ = crate::mr_delete_edge("U6d2f25cc4264","B79efabc4d8bf","");
        let _ = crate::mr_delete_edge("U9c1051c9bb99","U000000000000","");
        let _ = crate::mr_delete_edge("B4115d364e05b","Uf8bf10852d43","");
        let _ = crate::mr_delete_edge("Ue40b938f47a4","B8120aa1edccb","");
        let _ = crate::mr_delete_edge("U4ff50cbb890f","U000000000000","");
        let _ = crate::mr_delete_edge("U660f0dfe3117","U000000000000","");
        let _ = crate::mr_delete_edge("U26aca0e369c7","U000000000000","");
        let _ = crate::mr_delete_edge("Uef7fbf45ef11","U6d2f25cc4264","");
        let _ = crate::mr_delete_edge("Uc1158424318a","B499bfc56e77b","");
        let _ = crate::mr_delete_edge("U499f24158a40","Cd172fb3fdc41","");
        let _ = crate::mr_delete_edge("Uaa4e2be7a87a","C0b19d314485e","");
        let _ = crate::mr_delete_edge("U6d2f25cc4264","B5eb4c6be535a","");
        let _ = crate::mr_delete_edge("U17789c126682","U000000000000","");
        let _ = crate::mr_delete_edge("Ubd48a3c8df1e","U000000000000","");
        let _ = crate::mr_delete_edge("Bad1c69de7837","Uad577360d968","");
        let _ = crate::mr_delete_edge("U8a78048d60f7","Bf34ee3bfc12b","");
        let _ = crate::mr_delete_edge("U80e22da6d8c4","C2bbd63b00224","");
        let _ = crate::mr_delete_edge("U35eb26fc07b4","Cb117f464e558","");
        let _ = crate::mr_delete_edge("U57a6591c7ee1","U000000000000","");
        let _ = crate::mr_delete_edge("U8aa2e2623fa5","U000000000000","");
        let _ = crate::mr_delete_edge("U8a78048d60f7","Cc42c3eeb9d20","");
        let _ = crate::mr_delete_edge("C78ad459d3b81","U9a2c85753a6d","");
        let _ = crate::mr_delete_edge("Uf2b0a6b1d423","C3fd1fdebe0e9","");
        let _ = crate::mr_delete_edge("Uaa4e2be7a87a","B7f628ad203b5","");
        let _ = crate::mr_delete_edge("U8b70c7c00136","U000000000000","");
        let _ = crate::mr_delete_edge("Uf75d4cbe5430","U000000000000","");
        let _ = crate::mr_delete_edge("U8a78048d60f7","Be2b46c17f1da","");
        let _ = crate::mr_delete_edge("U84f274f30e33","U000000000000","");
        let _ = crate::mr_delete_edge("U26451935eec8","U000000000000","");
        let _ = crate::mr_delete_edge("U83e829a2e822","Bad1c69de7837","");
        let _ = crate::mr_delete_edge("U80e22da6d8c4","C35678a54ef5f","");
        let _ = crate::mr_delete_edge("U5d0cd6daa146","U000000000000","");
        let _ = crate::mr_delete_edge("Uf5096f6ab14e","U7a8d8324441d","");
        let _ = crate::mr_delete_edge("C9462ca240ceb","Uf5096f6ab14e","");
        let _ = crate::mr_delete_edge("U9a2c85753a6d","C4893c40e481d","");
        let _ = crate::mr_delete_edge("U18a178de1dfb","Bf34ee3bfc12b","");
        let _ = crate::mr_delete_edge("U3de789cac826","C3c17b70c3357","");
        let _ = crate::mr_delete_edge("U6661263fb410","Ce49159fe9d01","");
        let _ = crate::mr_delete_edge("U09cf1f359454","B310b66ab31fb","");
        let _ = crate::mr_delete_edge("Ua4041a93bdf4","U000000000000","");
        let _ = crate::mr_delete_edge("U389f9f24b31c","C6aebafa4fe8e","");
        let _ = crate::mr_delete_edge("U21769235b28d","C6d52e861b366","");
        let _ = crate::mr_delete_edge("Uad577360d968","C6a2263dc469e","");
        let _ = crate::mr_delete_edge("Udece0afd9a8b","Bad1c69de7837","");
        let _ = crate::mr_delete_edge("U1e41b5f3adff","U000000000000","");
        let _ = crate::mr_delete_edge("U9a2c85753a6d","C6aebafa4fe8e","");
        let _ = crate::mr_delete_edge("Ueb139752b907","B1533941e2773","");
        let _ = crate::mr_delete_edge("Udece0afd9a8b","U1c285703fc63","");
        let _ = crate::mr_delete_edge("U09cf1f359454","Be2b46c17f1da","");
        let _ = crate::mr_delete_edge("Ub01f4ad1b03f","B92e4a185c654","");
        let _ = crate::mr_delete_edge("Cbcf72c7e6061","U0f63ee3db59b","");
        let _ = crate::mr_delete_edge("Cf92f90725ffc","U6661263fb410","");
        let _ = crate::mr_delete_edge("U6d2f25cc4264","U000000000000","");
        let _ = crate::mr_delete_edge("U6106ae1092fa","U000000000000","");
        let _ = crate::mr_delete_edge("U037b51a34f3c","U000000000000","");
        let _ = crate::mr_delete_edge("U526f361717a8","B9c01ce5718d1","");
        let _ = crate::mr_delete_edge("Ud9df8116deba","B310b66ab31fb","");
        let _ = crate::mr_delete_edge("U6661263fb410","C22e1102411ce","");
        let _ = crate::mr_delete_edge("Udc7c82928598","U000000000000","");
        let _ = crate::mr_delete_edge("U22ad914a7065","U000000000000","");
        let _ = crate::mr_delete_edge("U21769235b28d","C8ece5c618ac1","");
        let _ = crate::mr_delete_edge("U99a0f1f7e6ee","B10d3f548efc4","");
        let _ = crate::mr_delete_edge("Ce06bda6030fe","U362d375c067c","");
        let _ = crate::mr_delete_edge("U8a78048d60f7","Bf3a0a1165271","");
        let _ = crate::mr_delete_edge("Uc44834086c03","U000000000000","");
        let _ = crate::mr_delete_edge("Ue55b928fa8dd","U000000000000","");
        let _ = crate::mr_delete_edge("U7a54f2f24cf6","U000000000000","");
        let _ = crate::mr_delete_edge("C5167c9b3d347","U362d375c067c","");
        let _ = crate::mr_delete_edge("U6727ddef0614","U000000000000","");
        let _ = crate::mr_delete_edge("Ubeded808a9c0","U000000000000","");
        let _ = crate::mr_delete_edge("C613f00c1333c","U80e22da6d8c4","");
        let _ = crate::mr_delete_edge("U8a78048d60f7","B0a87a669fc28","");
        let _ = crate::mr_delete_edge("U6240251593cd","U000000000000","");
        let _ = crate::mr_delete_edge("Uef7fbf45ef11","C588ffef22463","");
        let _ = crate::mr_delete_edge("Uc1158424318a","C9028c7415403","");
        let _ = crate::mr_delete_edge("Ue40b938f47a4","B944097cdd968","");
        let _ = crate::mr_delete_edge("Ub01f4ad1b03f","B310b66ab31fb","");
        let _ = crate::mr_delete_edge("U016217c34c6e","U80e22da6d8c4","");
        let _ = crate::mr_delete_edge("U362d375c067c","C5060d0101429","");
        let _ = crate::mr_delete_edge("U9a2c85753a6d","Ce1a7d8996eb0","");
        let _ = crate::mr_delete_edge("U43dcf522b4dd","U000000000000","");
        let _ = crate::mr_delete_edge("Uad577360d968","C399b6349ab02","");
        let _ = crate::mr_delete_edge("U9a2c85753a6d","C6a2263dc469e","");
        let _ = crate::mr_delete_edge("Ud2c791d9e879","U000000000000","");
        let _ = crate::mr_delete_edge("C992d8370db6b","U6d2f25cc4264","");
        let _ = crate::mr_delete_edge("Uf6ce05bc4e5a","B9c01ce5718d1","");
        let _ = crate::mr_delete_edge("U016217c34c6e","C15d8dfaceb75","");
        let _ = crate::mr_delete_edge("Ub93799d9400e","B491d307dfe01","");
        let _ = crate::mr_delete_edge("Ubd9c1e76bb53","U000000000000","");
        let _ = crate::mr_delete_edge("U6d2f25cc4264","Cac6ca02355da","");
        let _ = crate::mr_delete_edge("U389f9f24b31c","C4f2dafca724f","");
        let _ = crate::mr_delete_edge("Ua6dfa92ad74d","U000000000000","");
        let _ = crate::mr_delete_edge("U0c17798eaab4","Uad577360d968","");
        let _ = crate::mr_delete_edge("U0667457dabfe","U000000000000","");
        let _ = crate::mr_delete_edge("U8842ed397bb7","C8c753f46c014","");
        let _ = crate::mr_delete_edge("U09cf1f359454","B73a44e2bbd44","");
        let _ = crate::mr_delete_edge("U6d2f25cc4264","C8d80016b8292","");
        let _ = crate::mr_delete_edge("Ufa76b4bb3c95","U000000000000","");
        let _ = crate::mr_delete_edge("C7a807e462b65","Ud7002ae5a86c","");
        let _ = crate::mr_delete_edge("C481cd737c873","U21769235b28d","");
        let _ = crate::mr_delete_edge("Ub786ef7c9e9f","U000000000000","");
        let _ = crate::mr_delete_edge("Uf3b5141d73f3","B9c01ce5718d1","");
        let _ = crate::mr_delete_edge("U430a8328643b","U000000000000","");
        let _ = crate::mr_delete_edge("U72f88cf28226","U499f24158a40","");
        let _ = crate::mr_delete_edge("Bd49e3dac97b0","Uadeb43da4abb","");
        let _ = crate::mr_delete_edge("U83e829a2e822","U000000000000","");
        let _ = crate::mr_delete_edge("C0166be581dd4","U499f24158a40","");
        let _ = crate::mr_delete_edge("Cd172fb3fdc41","U499f24158a40","");
        let _ = crate::mr_delete_edge("U79466f73dc0c","U01814d1ec9ff","");
        let _ = crate::mr_delete_edge("U362d375c067c","C5167c9b3d347","");
        let _ = crate::mr_delete_edge("Ue6cc7bfa0efd","Bed5126bc655d","");
        let _ = crate::mr_delete_edge("U8842ed397bb7","C789dceb76123","");
        let _ = crate::mr_delete_edge("Uda0a7acaeb90","U000000000000","");
        let _ = crate::mr_delete_edge("U499f24158a40","Ccbd85b8513f3","");
        let _ = crate::mr_delete_edge("U0a227036e790","U000000000000","");
        let _ = crate::mr_delete_edge("Cf77494dc63d7","U38fdca6685ca","");
        let _ = crate::mr_delete_edge("U9a89e0679dec","B0e230e9108dd","");
        let _ = crate::mr_delete_edge("Cd4417a5d718e","Ub93799d9400e","");
        let _ = crate::mr_delete_edge("U7553cc7bb536","U000000000000","");
        let _ = crate::mr_delete_edge("Ub01f4ad1b03f","Ud9df8116deba","");
        let _ = crate::mr_delete_edge("U5f7ff9cb9304","U000000000000","");
        let _ = crate::mr_delete_edge("Uee0fbe261b7f","U000000000000","");
        let _ = crate::mr_delete_edge("Uef7fbf45ef11","U000000000000","");
        let _ = crate::mr_delete_edge("Cb14487d862b3","Uf5096f6ab14e","");
        let _ = crate::mr_delete_edge("Ud04c89aaf453","U000000000000","");
        let _ = crate::mr_delete_edge("Uaa4e2be7a87a","C588ffef22463","");
        let _ = crate::mr_delete_edge("U0c17798eaab4","C588ffef22463","");
        let _ = crate::mr_delete_edge("Uaa4e2be7a87a","C78d6fac93d00","");
        let _ = crate::mr_delete_edge("Ue3b747447a90","U000000000000","");
        let _ = crate::mr_delete_edge("U59abf06369c3","Be2b46c17f1da","");
        let _ = crate::mr_delete_edge("Ucb84c094edba","B491d307dfe01","");
        let _ = crate::mr_delete_edge("Uc2bfe7e7308d","U000000000000","");
        let _ = crate::mr_delete_edge("Ubeded808a9c0","B7f628ad203b5","");
        let _ = crate::mr_delete_edge("Uac897fe92894","U000000000000","");
        let _ = crate::mr_delete_edge("U80e22da6d8c4","Ue7a29d5409f2","");
        let _ = crate::mr_delete_edge("U4ba2e4e81c0e","Caa62fc21e191","");
        let _ = crate::mr_delete_edge("Ub93799d9400e","Ccae34b3da05e","");
        let _ = crate::mr_delete_edge("U9e42f6dab85a","U000000000000","");
        let _ = crate::mr_delete_edge("U0e6659929c53","C6d52e861b366","");
        let _ = crate::mr_delete_edge("U38fdca6685ca","C0f834110f700","");
        let _ = crate::mr_delete_edge("B92e4a185c654","U41784ed376c3","");
        let _ = crate::mr_delete_edge("B5a1c1d3d0140","Uc3c31b8a022f","");
        let _ = crate::mr_delete_edge("C6a2263dc469e","Uf2b0a6b1d423","");
        let _ = crate::mr_delete_edge("U9a89e0679dec","Cbce32a9b256a","");
        let _ = crate::mr_delete_edge("Uf5096f6ab14e","C3e84102071d1","");
        let _ = crate::mr_delete_edge("Uef7fbf45ef11","C94bb73c10a06","");
        let _ = crate::mr_delete_edge("C4f2dafca724f","U7a8d8324441d","");
        let _ = crate::mr_delete_edge("U4f530cfe771e","B7f628ad203b5","");
        let _ = crate::mr_delete_edge("Ub01f4ad1b03f","B75a44a52fa29","");
        let _ = crate::mr_delete_edge("Ubd93205079e9","U000000000000","");
        let _ = crate::mr_delete_edge("U9a2c85753a6d","U000000000000","");
        let _ = crate::mr_delete_edge("U7bd2e29031a4","U000000000000","");
        let _ = crate::mr_delete_edge("Ud5f1a29622d1","B7f628ad203b5","");
        let _ = crate::mr_delete_edge("U8676859527f3","U000000000000","");
        let _ = crate::mr_delete_edge("Cbbf2df46955b","U7a8d8324441d","");
        let _ = crate::mr_delete_edge("B5eb4c6be535a","Uad577360d968","");
        let _ = crate::mr_delete_edge("U01814d1ec9ff","C7062e90f7422","");
        let _ = crate::mr_delete_edge("U41784ed376c3","U000000000000","");
        let _ = crate::mr_delete_edge("U99a0f1f7e6ee","C279db553a831","");
        let _ = crate::mr_delete_edge("C15d8dfaceb75","U9e42f6dab85a","");
        let _ = crate::mr_delete_edge("Ca0a6aea6c82e","U016217c34c6e","");
        let _ = crate::mr_delete_edge("Uc3c31b8a022f","B5a1c1d3d0140","");
        let _ = crate::mr_delete_edge("U35eb26fc07b4","B7f628ad203b5","");
        let _ = crate::mr_delete_edge("U5f2702cc8ade","U000000000000","");
        let _ = crate::mr_delete_edge("U11456af7d414","U000000000000","");
        let _ = crate::mr_delete_edge("Ue40b938f47a4","Cb3c476a45037","");
        let _ = crate::mr_delete_edge("Uaa4e2be7a87a","C070e739180d6","");
        let _ = crate::mr_delete_edge("U8a78048d60f7","B8a531802473b","");
        let _ = crate::mr_delete_edge("U1df3e39ebe59","U000000000000","");
        let _ = crate::mr_delete_edge("U6661263fb410","Ccb7dc40f1513","");
        let _ = crate::mr_delete_edge("U8a78048d60f7","Cb07d467c1c5e","");
        let _ = crate::mr_delete_edge("C789dceb76123","U8842ed397bb7","");
        let _ = crate::mr_delete_edge("Uad577360d968","U389f9f24b31c","");
        let _ = crate::mr_delete_edge("U57b6f30fc663","U000000000000","");
        let _ = crate::mr_delete_edge("U2d8ff859cca4","U000000000000","");
        let _ = crate::mr_delete_edge("C54972a5fbc16","U499f24158a40","");
        let _ = crate::mr_delete_edge("U052641f28245","U000000000000","");
        let _ = crate::mr_delete_edge("Bb78026d99388","U9a89e0679dec","");
        let _ = crate::mr_delete_edge("U389f9f24b31c","B25c85fe0df2d","");
        let _ = crate::mr_delete_edge("Uc244d6132650","U000000000000","");
        let _ = crate::mr_delete_edge("U79466f73dc0c","Bad1c69de7837","");
        let _ = crate::mr_delete_edge("U622a649ddf56","U000000000000","");
        let _ = crate::mr_delete_edge("U8a78048d60f7","Ba3c4a280657d","");
        let _ = crate::mr_delete_edge("C6d52e861b366","U21769235b28d","");
        let _ = crate::mr_delete_edge("U1eedef3e4d10","U000000000000","");
        let _ = crate::mr_delete_edge("U8a78048d60f7","C6acd550a4ef3","");
        let _ = crate::mr_delete_edge("U80e22da6d8c4","B60d725feca77","");
        let _ = crate::mr_delete_edge("U1c285703fc63","B63fbe1427d09","");
        let _ = crate::mr_delete_edge("Uc5d62a177997","U000000000000","");
        let _ = crate::mr_delete_edge("U8aa2e2623fa5","C7c4d9ca4623e","");
        let _ = crate::mr_delete_edge("U1bcba4fd7175","C6d52e861b366","");
        let _ = crate::mr_delete_edge("C30e7409c2d5f","U80e22da6d8c4","");
        let _ = crate::mr_delete_edge("Ub01f4ad1b03f","B491d307dfe01","");
        let _ = crate::mr_delete_edge("U77f496546efa","U000000000000","");
        let _ = crate::mr_delete_edge("U798f0a5b78f0","U000000000000","");
        let _ = crate::mr_delete_edge("C801f204d0da8","U21769235b28d","");
        let _ = crate::mr_delete_edge("C5782d559baad","U0cd6bd2dde4f","");
        let _ = crate::mr_delete_edge("U41784ed376c3","B92e4a185c654","");
        let _ = crate::mr_delete_edge("U26aca0e369c7","Cb117f464e558","");
        let _ = crate::mr_delete_edge("U704bd6ecde75","Cdd49e516723a","");
        let _ = crate::mr_delete_edge("Ucbca544d500f","U000000000000","");
        let _ = crate::mr_delete_edge("Ucbd309d6fcc0","B5e7178dd70bb","");
        let _ = crate::mr_delete_edge("Ue2570414501b","U000000000000","");
        let _ = crate::mr_delete_edge("Uf8bf10852d43","B19d70698e3d8","");
        let _ = crate::mr_delete_edge("U8fc7861a79b9","U000000000000","");
        let _ = crate::mr_delete_edge("U5502925dfe14","U000000000000","");
        let _ = crate::mr_delete_edge("C5060d0101429","U362d375c067c","");
        let _ = crate::mr_delete_edge("B253177f84f08","Uf8bf10852d43","");
        let _ = crate::mr_delete_edge("U34252014c05b","Bb1e3630d2f4a","");
        let _ = crate::mr_delete_edge("U80e22da6d8c4","Cb14487d862b3","");
        let _ = crate::mr_delete_edge("U707f9ed34910","U000000000000","");
        let _ = crate::mr_delete_edge("Cc01e00342d63","U6661263fb410","");
        let _ = crate::mr_delete_edge("C10872dc9b863","U499f24158a40","");
        let _ = crate::mr_delete_edge("U8a78048d60f7","Be29b4af3f7a5","");
        let _ = crate::mr_delete_edge("U499f24158a40","C4818c4ed20bf","");
        let _ = crate::mr_delete_edge("C3fd1fdebe0e9","U7a8d8324441d","");
        let _ = crate::mr_delete_edge("U11456af7d414","Bad1c69de7837","");
        let _ = crate::mr_delete_edge("U6a774cf456f7","U000000000000","");
        let _ = crate::mr_delete_edge("U80e22da6d8c4","B45d72e29f004","");
        let _ = crate::mr_delete_edge("U8a78048d60f7","B3b3f2ecde430","");
        let _ = crate::mr_delete_edge("U1bcba4fd7175","Bc4addf09b79f","");
    }

    #[pg_test]
    fn test_zerorec() {
        put_testing_edges();

        let _ = crate::mr_zerorec().unwrap();

        delete_testing_edges();
    }

    #[pg_test]
    fn test_service() {
        let ver = crate::mr_service();

        //  check if ver is in form "X.Y.Z"
        assert_eq!(ver.split(".").map(|x|
            x.parse::<u32>().unwrap()
        ).count(), 3);
    }

    #[pg_test]
    fn test_edge_uncontexted() {
        let res = crate::mr_put_edge("U1", "U2", 1.0, "").unwrap();

        let n = res.map(|x| {
            assert_eq!(x.0, "U1");
            assert_eq!(x.1, "U2");
            assert_eq!(x.2, 1.0);
        }).count();

        assert_eq!(n, 1);

        let _ = crate::mr_delete_edge("U1", "U2", "");
    }

    #[pg_test]
    fn test_edge_contexted() {
        let res = crate::mr_put_edge("U1", "U2", 1.0, "X").unwrap();
        
        let n = res.map(|x| {
            assert_eq!(x.0, "U1");
            assert_eq!(x.1, "U2");
            assert_eq!(x.2, 1.0);
        }).count();

        assert_eq!(n, 1);

        let _ = crate::mr_delete_edge("U1", "U2", "X");
    }

    #[pg_test]
    fn test_null_context_is_sum() {
        let _ = crate::mr_put_edge("U1", "U2", 1.0, "X");
        let _ = crate::mr_put_edge("U1", "U2", 2.0, "Y");

        let res = crate::mr_edgelist("").unwrap();

        let n = res.map(|x| {
            assert_eq!(x.0, "U1");
            assert_eq!(x.1, "U2");
            assert_eq!(x.2, 3.0);
        }).count();

        assert_eq!(n, 1);

        let _ = crate::mr_delete_edge("U1", "U2", "X");
        let _ = crate::mr_delete_edge("U1", "U2", "Y");
    }


    #[pg_test]
    fn test_delete_contexted_edge() {
        let _ = crate::mr_put_edge("U1", "U2", 1.0, "X");
        let _ = crate::mr_put_edge("U1", "U2", 2.0, "Y");
        let _ = crate::mr_delete_edge("U1", "U2", "X");

        //  We should still have "Y" edge.
        let res = crate::mr_edgelist("").unwrap();

        let n = res.map(|x| {
            assert_eq!(x.0, "U1");
            assert_eq!(x.1, "U2");
            assert_eq!(x.2, 2.0);
        }).count();

        assert_eq!(n, 1);

        let _ = crate::mr_delete_edge("U1", "U2", "Y");
    }

    #[pg_test]
    fn test_null_context_invariant() {
        let _ = crate::mr_put_edge("U1", "U2", 1.0, "X");
        let _ = crate::mr_put_edge("U1", "U2", 2.0, "Y");

        //  Delete and put back again.
        let _ = crate::mr_delete_edge("U1", "U2", "X");
        let _ = crate::mr_put_edge("U1", "U2", 1.0, "X");

        let res = crate::mr_edgelist("").unwrap();

        let n = res.map(|x| {
            assert_eq!(x.0, "U1");
            assert_eq!(x.1, "U2");
            assert_eq!(x.2, 3.0);
        }).count();

        assert_eq!(n, 1);

        let _ = crate::mr_delete_edge("U1", "U2", "X");
        let _ = crate::mr_delete_edge("U1", "U2", "Y");
    }
}

#[cfg(test)]
pub mod pg_test {
    pub fn setup(_options: Vec<&str>) {
    }

    pub fn postgresql_conf_options() -> Vec<&'static str> {
        vec![]
    }
}
