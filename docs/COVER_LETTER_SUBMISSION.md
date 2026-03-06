# Cover Letter

Dear Editor,

Please consider our manuscript, **“Design and Evaluation of a Ledger-First Frequent Batch Auction Market System,”** for publication in **[Venue Name]**.

This submission presents a modular market infrastructure prototype that combines frequent batch auction matching, ledger-first double-entry settlement, explicit risk-state controls, and event-driven system integration. The paper is positioned as a systems artifact rather than a product description. Its central contribution is the combination of a clearly stated research question, an explicit batch-clearing algorithm, measurable baseline-versus-batching behavior, and executable correctness guarantees, including deterministic clearing-price selection, matched-volume conservation, replay-safe ledger mutation, and monotonic operational restrictions under escalating risk controls.

We believe the manuscript fits **[Venue Name]** for three reasons. First, it studies a core market design question, namely how discrete-time batch matching changes the engineering tradeoff between batching and responsiveness relative to an immediate-clearing surrogate. Second, it contributes an implementation-focused systems perspective by separating matching, settlement, and risk logic into independently testable modules. Third, it treats correctness claims as part of the artifact itself through invariant tests and reproducible benchmark outputs, which should be of interest to readers working on market infrastructure, financial systems, and dependable event-driven architectures.

The manuscript reports synthetic evaluation results using an immediate-clearing surrogate baseline and FBA windows of 100 ms, 250 ms, and 500 ms. Under the current benchmark scenario of 200 buy/sell pairs in a single market/outcome bucket, the system reaches 144,744.0 orders/s in the immediate surrogate mode, 4,978.0 orders/s at 100 ms FBA, and 833.4 orders/s at 500 ms FBA, while preserving deterministic settlement semantics. The paper also discusses the current limitations of the prototype, including its single-node evaluation setting, simplified baseline, and in-memory write-ahead log, so that claims remain aligned with the actual scope of the implementation.

This manuscript is original, is not under consideration elsewhere, and has not been previously published in substantially similar form. All authors have approved the submission. We would be happy to provide any additional material that may assist the review process.

Thank you for your consideration.

Sincerely,

**[Author Name]**  
**[Affiliation]**  
**[Email]**
