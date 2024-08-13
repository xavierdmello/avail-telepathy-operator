# Avail Telepathy Operator
The operator from [`SP1 Telepathy`](https://github.com/succinctlabs/sp1-telepathy) (`script/bin/operator.rs`) adapted for operating Avail.

The logic remains the same, with placeholders for reading and writing to Avail using `avail-subxt` instead of a smart contract on Ethereum.

For more context, please see https://github.com/availproject/avail/pull/630

## Run operator
The code ([`/script/src/operator.rs`](https://github.com/xavierdmello/avail-telepathy-operator/blob/master/script/src/operator.rs)) compiles and should run given the placeholder functions and values are filled in. They are marked by comments beginning with `// TODO - AVAIL:`
1. `cd ./script`
2. `cargo run`


