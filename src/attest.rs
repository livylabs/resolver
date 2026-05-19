//! Experimental attestation helpers for provenance work.

use livy_tee::{Attestation, Livy};


async fn mockattestation(input: &str)  {
    let livy = Livy::new("Data Source");
    let mut builder = livy.attest();
    // I'll think that i would wanna get a nonce from an rpc 
    builder.commit(&input).nonce(1);
    let attestation = builder.finalize().await.expect("fail");
    println!("{}" , serde_json::to_string_pretty(&attestation).expect("fail"));

}

async fn track_attest(witness: Attestation) {
    // Post attestation to our provenance layer 
}


