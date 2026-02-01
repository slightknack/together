+++
model = "claude-opus-4-5"
created = 2026-01-30
modified = 2026-01-30
driver = "Isaac Clayton"
+++

# Vision

The web is broken. "The plains of collective consciousness are ravaged by seas of memetic tofu searching for people whose thoughts they can allocate to the highest bidder." The alternative is "tight-knit communities of writers and thinkers and artists and photo-sharers and moms and kids and dads grounded in reality." Private, local-first, federated.

This vision drives a stack of projects. At the bottom is a programming language. In the middle is a synchronization primitive. At the top is a server for home-grown software. Each layer makes the next possible.

## Affetto

Affetto is a programming language with "affine algebraic effects. Affine Effects. Affetto." Algebraic effects "let you project frameworks into libraries and libraries into frameworks; these are inversions of control."

Algebraic effects let you write code that looks like direct style but can be interpreted differently by the caller. A function that reads from database does not know if that database is local, remote, or replicated. The effect handler decides. This is the inversion of control that turns frameworks into libraries.

Affetto compiles to "c99 and wasm components; the perfect target for libraries. If someone wanted to write SQLite today, I hope they would choose affetto." WebAssembly components are the universal compilation target. C99 is the portable bedrock. Both are targets for code that wants to be linked, not deployed.

## Together

Together is SQLite for CRDTs. SQLite succeeded because it is a library, not a server. You link it in, you have a database. No configuration, no deployment, no network. Together should be the same for collaborative state: link it in, you have sync. The signed append-only logs give authenticated ordering. The CRDTs give conflict-free merging. The combination is like git but for arbitrary data structures, where merges always succeed.

Together is the synchronization primitive that makes the rest possible. If together is good enough, the hard part of distributed applications becomes easy: you establish a together endpoint and the data stays consistent across devices and users without the application author thinking about sync. The application becomes a view over a CRDT.

The architecture uses "single writer cores that are bundled in some way (doing authentication with a changing membership set) and then merging those cores." A core is "an authenticated signed append only log." Each participant has their own core. A bundle is a set of cores with a membership protocol. Merge happens across cores using CRDT properties. The single-writer constraint keeps each core simple and verifiable. The bundle handles multi-writer collaboration.

If together exposes its operations as effects, an affetto program could be transparently local or distributed depending on how you run it. Together is written in Rust for now, as affetto is not finished yet.

## Home

Home is "a server that lets you write home-grown software. It's a bit like git, in that there is version control. It's a bit like docker or kubernetes, as it can run applications and restart or migrate them as needed." It "runs webassembly components with capability-based security" and "has a translation layer to intercept calls to other components required, and can run apps distributed across machines by treating WIT as the RPC boundary."

The WIT-as-RPC-boundary idea is powerful. If component boundaries are already capability-typed interfaces, distribution becomes routing. The runtime decides whether a call crosses a thread, a process, or a network. Together handles the state; WIT handles the calls.

This distribution should be "completely and totally transparent to the end user." But the system should "be usable by someone who is not a software engineer, by the help of someone who is."

That help may come from AI. The goal is to "make the whole system legible to AI agents, who can use these very strong local-first primitives to zero-shot arbitrary applications." If together is good enough, "we can just establish a together endpoint from the frontend to the backend and implement e.g. an instagram-like photo sharing app in a single html page or simple website."

Home should "easily federate; as in, if you run your own instance, you can 1) copy apps running on home and 2) collaborate on instances of the same app with the features of together."

## Origins

"Broadly, a long time ago, I started a project called Solidarity, with a specific idea called kitbag." The thread runs through work at tonari, a Japanese company building technology to bridge physical spaces, and at Zed, where the extension system uses wasmtime and the component model.

The goal remains: a better web.

---

All quotes in this document are from the driver, verbatim.
