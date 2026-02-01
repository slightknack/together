const fs = require('fs');
const zlib = require('zlib');
const { Model } = require('json-joy/lib/json-crdt');
const { s } = require('json-joy/lib/json-crdt-patch');

const traces = [
    ['sveltecomponent', '../../data/editing-traces/sequential_traces/ascii_only/sveltecomponent.json.gz'],
    ['rustcode', '../../data/editing-traces/sequential_traces/ascii_only/rustcode.json.gz'],
    ['seph-blog1', '../../data/editing-traces/sequential_traces/ascii_only/seph-blog1.json.gz'],
    ['automerge-paper', '../../data/editing-traces/sequential_traces/ascii_only/automerge-paper.json.gz'],
];

function loadTrace(path) {
    const compressed = fs.readFileSync(path);
    const json = zlib.gunzipSync(compressed).toString('utf8');
    return JSON.parse(json);
}

function replayJsonJoy(data) {
    // Create model with string schema
    const model = Model.create(s.str(''));
    const str = model.api.str([]);
    
    for (const txn of data.txns) {
        for (const [pos, del, ins] of txn.patches) {
            if (del > 0) {
                str.del(pos, del);
            }
            if (ins.length > 0) {
                str.ins(pos, ins);
            }
        }
    }
    
    return str.view();
}

function benchmark(name, iterations, fn) {
    // Warmup
    for (let i = 0; i < 2; i++) {
        fn();
    }
    
    // Collect timings
    const times = [];
    for (let i = 0; i < iterations; i++) {
        const start = process.hrtime.bigint();
        fn();
        const end = process.hrtime.bigint();
        times.push(Number(end - start) / 1e6); // ms
    }
    
    // Return median
    times.sort((a, b) => a - b);
    return times[Math.floor(iterations / 2)];
}

async function main() {
    const iterations = 10;
    
    console.log('Loading traces...');
    const datasets = traces.map(([name, path]) => {
        const data = loadTrace(path);
        const patchCount = data.txns.reduce((acc, txn) => acc + txn.patches.length, 0);
        console.log(`  ${name}: ${patchCount} patches`);
        return [name, data];
    });
    
    console.log('\nVerifying correctness...');
    for (const [name, data] of datasets) {
        const result = replayJsonJoy(data);
        if (result !== data.endContent) {
            console.error(`  ${name}: MISMATCH!`);
            console.error(`    Expected length: ${data.endContent.length}`);
            console.error(`    Got length: ${result.length}`);
        } else {
            console.log(`  ${name}: correct`);
        }
    }
    
    console.log(`\nRunning benchmarks (${iterations} iterations, reporting median)...\n`);
    
    const results = [];
    for (const [name, data] of datasets) {
        console.log(`Benchmarking ${name}...`);
        const time = benchmark(name, iterations, () => replayJsonJoy(data));
        console.log(`  json-joy: ${time.toFixed(2)} ms`);
        results.push([name, time]);
    }
    
    console.log('\n=== Results Table ===\n');
    console.log('| Trace | json-joy (ms) |');
    console.log('|-------|--------------|');
    for (const [name, time] of results) {
        console.log(`| \`${name}\` | ${time.toFixed(2)} |`);
    }
}

main().catch(console.error);
