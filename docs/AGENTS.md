# Writing Style for Docs

## Audience

Engineers working **on** the alien/ codebase — reading crates, fixing bugs, adding features.

Each doc maps to one or more crates. If you can't name the crate a doc describes, it doesn't belong here.

Assume readers have read `ARCHITECTURE.md` and understand the codebase structure.

Docs are grouped by topic in numbered directories. Within each directory, files are numbered in reading order.

## Principles

1. **Guide before reference.** Every doc opens by connecting to what the reader just read and showing what this crate does in one concrete sentence. The first code example should be the simplest possible use — not a full struct definition. Struct definitions, trait signatures, and implementation details come after the reader has the concept. Never open a doc with a trait or struct definition.

2. **Clear, concise, focused.** One idea per sentence. Short paragraphs. No filler.

3. **Simple before complex.** Start with the simplest example. Build up incrementally. Full examples come last, after concepts are explained.

4. **One concept per section.** Each section answers one question. Don't mix concepts.

5. **Concrete over abstract.** Show code or examples immediately when introducing a concept. Minimal code - only what's needed to illustrate the point.

6. **Desired state, not current state.** Describe how things should work. Don't mention previous implementations or migrations.

7. **Consistent terminology.** Match names in docs to names in code.

8. **Active voice.** "The runtime starts the process" not "The process is started by the runtime."

## Terminology

- **Developer** not "vendor" - the person/company building and shipping software with Alien
- **Ship updates** - prefer this over "deploy new versions." Alien brings the web experience (ship every week like Vercel) to remote environments you don't control.
- **Platform** not "cloud" - Alien supports AWS, GCP, Azure, Kubernetes, and Local. Say "platform" when speaking generically. Only say "cloud" when specifically talking about AWS/GCP/Azure.
- **Control plane** - the generic term for whatever calls `alien-deployment` and persists state.
- **Remote environment** - where the deployment runs (customer cloud, K8s cluster, local machine).

## Don'ts

- Don't dump everything at once. Build up.
- Don't repeat information across docs. Link instead.
- Don't explain what something *isn't*. Explain what it *is*.
- Don't hedge. If something is true, state it. If uncertain, say why.
- Don't use "basically," "simply," "just," or "obviously."
- Don't use "vendor" - use "developer" instead.
- Don't reference specific control plane components by name. Use generic terms like "control plane" or "the caller." docs must be self-contained.
- Don't write conceptual product docs (environment types, deployment methods, SDK tutorials).
- Don't include API references or lists of services/methods. The reader can find those in the code. Docs teach *how* and *why*, not *what exists*.
