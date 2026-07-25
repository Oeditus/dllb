# dllb v1.0.0: Multi-Model NoSQL Database for Code Intelligence & Hybrid RAG

We are excited to announce the release of **dllb v1.0.0** (Prototype Phase), a multi-model NoSQL database management system built from scratch in Rust. Natively combining **documents**, **graphs**, **full-text search**, and **vector embeddings** into a single, unified database engine, `dllb` eliminates the complexity, latency, and synchronization overhead of polyglot persistence.

Developed specifically for **Code Intelligence** and advanced **Retrieval-Augmented Generation (RAG)** systems, `dllb` serves as the high-performance storage substrate for structural AST parsing, call-graph representation, code-aware search, and semantic similarity mappings.

---

## Key Advantages & Core Architecture

### 1. Unified Multi-Model Substrate
Traditional architectures require stitching together separate databases (e.g., MongoDB for profiles, Neo4j for graphs, Elasticsearch for full-text search, and Pinecone for vector search). `dllb` replaces this with a single sorted byte space, storing all four models as structured key-value pairs. 
- **ACID & MVCC Storage**: Powered by [redb](https://github.com/cberner/redb)—a pure-Rust, copy-on-write B-tree database with serializable ACID transactions, non-blocking MVCC readers, and zero C dependencies.
- **Unified Querying**: Cross-model SQL-like queries can merge vector similarity searches with graph traversals and full-text filters in a single execution plan.

### 2. Type-Tagged Binary Key Encoding
All data lives in one sorted keyspace. Type tags in keys distinguish models:
- `!` (0x21) — **Metadata**: Schema definitions and database catalogs.
- `*` (0x2A) — **Document**: Primary schemaless or schemafull JSON/MessagePack records.
- `+` (0x2B) — **Index**: Secondary indexes, HNSW vector links, and Tantivy inverted indexes.
- `~` (0x7E) — **Graph**: Bidirectional edge pointers.

By using structured keys (`[namespace][0x00][database][0x00][table][tag][remainder]`), every operation—graph traversals, secondary index scans, and document lookups—reduces to **prefix range scans** over contiguous byte slices.

```
[Outgoing Edge]  ~src_vertex\0>edge_type\0dst_vertex  --> MessagePack properties
[Incoming Edge]  ~dst_vertex\0<edge_type\0src_vertex  --> [Empty Pointer]
```

### 3. Actor-Based Fault Tolerance
Using the [joerl](https://crates.io/crates/joerl) actor framework (Erlang/OTP-inspired supervision trees in Rust), `dllb` guarantees fault-tolerant concurrency:
- **Write Serialization**: Bounded write operations are serialized through a `StorageWriter` GenServer actor, avoiding concurrent transaction conflicts.
- **Direct Reads**: Reads bypass the actor mailbox, querying the database directly using lock-free read transactions (zero mailbox overhead).
- **Supervised Subsystems**: FTS indexes (`FtsActor`), vector graphs (`HnswActor`), and client connections (`ConnectionActor`) run in separate, monitored failure domains. If an actor crashes, its supervisor automatically recovers its state.

---

## Pre-Packaged Capabilities & Core Models

| Model | Underlying Technology | Highlighted Feature | Use Cases |
| :--- | :--- | :--- | :--- |
| **Documents** | MessagePack & B-Tree Indexes | Dynamic schemas, fast point lookups, secondary indexes. | Storing structured objects, AST nodes, configuration. |
| **Graphs** | Co-located bidirectional key regions | Multi-hop directed walks, community detection, PageRank. | Call graphs, dependency trees, inheritance hierarchies. |
| **Full-Text** | [Tantivy](https://github.com/quickwit-oss/tantivy) (Lucene-class) | BM25 relevance scoring, language stemming, stemming filters. | Searchable code snippets, docstring discovery, text match. |
| **Vectors** | In-memory HNSW index | Cosine, L2, dot product metrics, SIMD acceleration, soft deletes. | Semantic code lookup, RAG context matching, recommendation. |

---

## Code Intelligence: A First-Class Citizen

`dllb` is designed from the ground up as a first-class repository for **Abstract Syntax Trees (AST)** and code metadata:

1. **Predefined AST Schema**: The database defines a standard 13-field AST node schema containing file paths, line ranges, source code, docstrings, and multiple embeddings (source, structural, and docstring vectors).
2. **Code-Aware Tokenizer**: A custom full-text analyzer that splits `camelCase` and `snake_case` boundaries and filters out syntax noise (e.g., stripping 70+ programming keywords like `async`, `fn`, `public`) to optimize search indexes.
3. **Local Call-Graph Resolution**: During ingestion, function calls within a file are matched against local function definitions. If found, graph edges (`calls`) point directly to the resolved document IDs (e.g., `parser.ex::parse/2`) rather than placeholder strings.
4. **SQL-Integrated AST Functions**:
   - `ast::complexity(ast_serialized)`: Computes the cyclomatic/cognitive complexity of a serialized AST subtree.
   - `ast::hash(ast_serialized)`: Produces a Zobrist structural hash of the code skeleton (ignoring variable names/literals) to detect exact clones.
   - `ast::similarity(ast1, ast2)`: Compares two ASTs using greedy best-match pairing to identify fuzzy clones.

---

## Elixir Driver & Connector: `dllb_ex`

The Elixir driver (`dllb_ex`) connects BEAM applications to the Rust database server over a high-performance, line-based TCP protocol.

### Features
- **NimblePool Integration**: Manages a connection pool of TCP sockets with dead-socket detection and automatic reconnection.
- **Composable Query Builder**: Composes raw queries using natural Elixir syntax.
- **Graph Analytics Support**: Built-in query functions for structural graph calculations (`pagerank`, `centrality`, `path`, `edges`, `components`, `communities`).
- **MetaAST Bridge**: Translates Metastatic AST 3-tuples (from the `metastatic` library) into binary-encoded document and edge mutations.

### Quick Example: Ingestion & Querying in Elixir

```elixir
# 1. Establish database connection pool
config :dllb,
  enabled: true,
  host: "127.0.0.1",
  port: 3009,
  pool_size: 10

# 2. Schema Bootstrap
{:ok, :bootstrapped} = Dllb.Schema.bootstrap(&Dllb.query/1)

# 3. Create a Document
query_str = Dllb.Query.create("user", %{name: "Alice", age: 30})
{:ok, %Dllb.Result.Created{id: record_id}} = Dllb.query(query_str)

# 4. Hybrid Search (FTS BM25 + HNSW Vector Similarity + Scope Filter)
search_query = Dllb.Query.hybrid_search(
  "ast_node", 
  "source_text", "parse tokens", 
  "source_embedding", [0.12, 0.07, 0.91],
  alpha: 0.6,
  limit: 5
)
{:ok, %Dllb.Result.Rows{data: results}} = Dllb.query(search_query)
```

---

## Real-World Applications: `Rageg` (Visual Code Intelligence)

`Rageg` is a Phoenix LiveView GUI that showcases the power of `dllb`'s multi-model approach in a Code Intelligence and RAG platform.

```
+--------------------+
|  Browser (D3.js)   |  Visualizes code dependencies & semantic spaces
+--------------------+
          |  (WebSockets)
+--------------------+
|  Phoenix LiveView  |  Web application logic
+--------------------+
          |  (Direct BEAM calls)
+--------------------+          (TCP)          +----------------------+
|  Ragex (in-BEAM)   |------------------------>|  dllb Server (Rust)  |
+--------------------+                         +----------------------+
```

### How `Rageg` Harnesses `dllb`

#### 1. Interactive Knowledge Graph Explorer
`Rageg` maps program structures into `dllb` graph tables (containment and calls). It queries `dllb` to get the topology and renders it using **D3.js**.
- Sizing and coloring nodes proportionally using metrics computed by `dllb`'s graph engine (`GRAPH PAGERANK` and `GRAPH CENTRALITY`).
- Grouping nodes in real time using `GRAPH COMMUNITIES`.

#### 2. Advanced Code Duplication & Complexity Audits
`Rageg` feeds AST structural representations to `dllb` and queries details using native SQL-AST functions. It detects clones (Type I-IV) by comparing `ast::hash` values and calculating `ast::similarity` across the codebase.

#### 3. Semantic Embedding Space
`Rageg` visualizes the code embedding space in a 2D scatter plot (using PCA coordinates of vector embeddings). It allows developers to:
- Input code fragments, request the embedding vector from `Ragex`, and execute a `VECTOR SEARCH` in `dllb`.
- Plot nearest neighbors dynamically (visualizing k-NN links directly on the scatter plot).

#### 4. Context-Aware RAG Chat
The chat interface leverages `dllb`'s hybrid search query capabilities to provide highly specific source code context to LLMs.
1. User asks: *"Where are the socket connection errors handled?"*
2. RAG Agent performs a **Hybrid Search** combining Tantivy text search (`"socket connection error"`) and HNSW vector similarity on the code embeddings, scoped to a specific project module:
   ```sql
   HYBRID SEARCH ast_node 
     TEXT source_text 'socket connection error' 
     VECTOR source_embedding $vector 
     WHERE project_path = '/my_app' 
     ALPHA 0.7 
     LIMIT 5
   ```
3. Using the returned AST nodes, the agent walks the call graph (`->calls->fn_node`) to find caller functions, injecting the complete structural relationship into the LLM prompt.

---

## Future Roadmap

As we progress beyond the prototype phase, the next stages of development include:
- **Reactivity**: Implementation of `LIVE SELECT` queries, real-time event notifications, and changefeeds.
- **Geo-Spatial Support**: R-tree index integration for GeoJSON querying.
- **Distributed Clustering**: Harnessing `joerl`'s node discovery and EPMD-based communication to support decentralized replicas and write-ahead log sharing.
- **Object-Oriented Storage**: Model inheritance and server-side computed/derived fields.

---

*`dllb` is licensed under the MIT License and developed as part of the [Oeditus](https://oeditus.com) static analysis ecosystem.*
