# MiniMax H3 video generation investigation

Status: proposal only; no runtime support or defaults changed. Sources checked September 5,
2026. No paid generation requests were made. Prices are USD list prices, not quotes, and
quality, latency, account eligibility, and end-to-end compatibility remain untested.

## Recommendation

Evaluate H3 as an **opt-in**, not a replacement for the default Veo workflow. Start with
OpenRouter for the smallest implementation and existing credentials; consider fal at 768P
if lower generation cost justifies a second provider integration. MiniMax direct and
Replicate are alternatives at 768P. Do not select a provider on model name alone: duration,
resolution, upscaling, price, and hosting differ.

Before enabling support, confirm commercial/territorial rights with the chosen provider,
implement model-specific capabilities and estimates, and obtain approval for a small paid
comparison. Native audio and reference editing are interesting H3 capabilities, but are not
immediate benefits for Reel Maestro's separately narrated, first-frame animation workflow.

## Cloud options and costs

These are image-to-video output prices. A 30-second example means five independently
generated 6-second clips, **not** one 30-second request. It excludes script, still images,
TTS, music, taxes, funding fees, input charges, and reruns. Different resolutions and
independently hosted variants are not quality-equivalent.

| Route / model or endpoint | Resolution | USD / generated second | 6s clip | 30s total | Integration |
| --- | --- | ---: | ---: | ---: | --- |
| [OpenRouter](https://openrouter.ai/minimax/hailuo-3), `minimax/hailuo-3` | 2K | $0.13 | $0.78 | $3.90 | Existing video client; capability/payload changes needed |
| [MiniMax direct](https://platform.minimax.io/docs/guides/pricing-paygo), `MiniMax-H3` | 768P / 2K | $0.08 / $0.13 | $0.48 / $0.78 | $2.40 / $3.90 | New V2 client and credentials |
| [fal](https://fal.ai/models/minimax/h3/image-to-video), `minimax/h3/image-to-video` | 480P / 768P / 2K / 4K | $0.05 / $0.06 / $0.13 / $0.16 | $0.30 / $0.36 / $0.78 / $0.96 | $1.50 / $1.80 / $3.90 / $4.80 | New queue client and credentials |
| [Replicate](https://replicate.com/minimax/h3), `minimax/h3` | 768P / 2K | $0.08 / $0.13 | $0.48 / $0.78 | $2.40 / $3.90 | New predictions client and credentials |
| [WaveSpeed official-model route](https://wavespeed.ai/models/minimax/h3/image-to-video), `minimax/h3/image-to-video` | 768p / 2k | $0.10 / $0.14 | $0.60 / $0.84 | $3.00 / $4.20 | New predictions client and credentials |
| [WaveSpeed open-weights route](https://wavespeed.ai/models/wavespeed-ai/minimax-h3/image-to-video), `wavespeed-ai/minimax-h3/image-to-video` | 480p / 540p / 768p / 1080p | $0.04 / $0.06 / $0.08 / $0.16 | $0.24 / $0.36 / $0.48 / $0.96 | $1.20 / $1.80 / $2.40 / $4.80 | Independently hosted; pricing/schema discrepancies below |

For comparison, the repository's existing Veo 3.1 Lite 720p estimate is $0.05/sec:
$0.30 per 6-second clip or $1.50 for these five clips. H3 through OpenRouter is 2.6 times
that estimated output cost, but delivers a different resolution. This is not a fresh
verification of Veo pricing or a quality benchmark.

### Provider-specific qualifications

- **OpenRouter:** its model page lists one upstream, MiniMax. Using an aggregator does not
  currently provide multi-provider failover for this model. The page documents 2K and
  5–15 seconds, unlike MiniMax direct's 768P/2K and 4–15 seconds. Do not assume that direct
  API capabilities are available through OpenRouter.
- **MiniMax direct:** [V2 create API](https://platform.minimax.io/docs/api-reference/video-generation-v2-create)
  uses `POST https://api.minimax.io/v2/video_generation`, `model: MiniMax-H3`, a `content`
  array, `resolution`, `duration`, and `ratio`. It returns `task_id`; this is not a base-URL
  substitution for OpenRouter. It requires pay-as-you-go access;
  [video packages](https://platform.minimax.io/docs/guides/pricing-video) explicitly exclude H3.
- **fal:** the page's introduction still describes only 2K. Its linked
  [OpenAPI schema](https://fal.ai/api/openapi/queue/openapi.json?endpoint_id=minimax/h3/image-to-video)
  documents 480P/768P native generation and 2K/4K upscaling of a 768P base result, with
  integer durations 5–15. Its current price footer lists the four rates above. Validate
  the selected tier at implementation time rather than relying on the older introduction.
- **Replicate:** its model README documents 4–15 seconds, first/last-frame input, reference
  inputs, and the two output rates. Extra reference charges were not established from
  that page; do not assume parity with MiniMax direct's full billing rules.
- **WaveSpeed:** the official route's introduction says fixed 2K, but its parameter and
  pricing tables also list 768p. The open-weights route's introduction says 5–15 seconds,
  while its detailed table says 3–15 and warns of frame-grid snapping (5s may yield about
  5.2s). Its playground showed $0.10/run while its README says $0.20 for 5s at 480p.
  The table above uses the explicit per-second README rates, not the conflicting widget;
  request a current quote/schema and confirm actual billing before adopting this route.
  WaveSpeed itself warns that documentation prices can be outdated.
- **H3 Max is separate:** do not alias it to H3. MiniMax lists 480P at $0.05/sec and 768P
  at $0.08/sec, with 5–15-second T2V/I2V. [fal describes H3 Max](https://fal.ai/minimax-h3-max)
  as a post-trained variant and advertises a limited introductory discount. Do not bake
  temporary discounts or its performance claims into H3 estimates.

### Additional charges and effective cost

[MiniMax's price sheet](https://platform.minimax.io/docs/guides/pricing-paygo) lists the first
five input images free, then $0.04/image; input audio is free; input video costs $0.08/sec
at 768P or $0.13/sec at 2K. Regenerating 768P output into 2K adds $0.05/output second and
rebills input materials under regeneration rates. Context-IR is separately priced at
$0.90/M input tokens and $3.60/M output tokens. These are direct-provider rules, not a
promise that every reseller bills identically. OpenRouter independently lists $0.04 per
reference image after the first five free.

The current first-frame workflow uses one image per request, so direct MiniMax's image
allowance covers it. There is no documented silent-output discount to apply to these H3
prices. Discarding generated audio during assembly does not imply a generation saving.

Budget by requested, rounded clip lengths rather than finished reel duration. At a
5-second minimum, ten 2-second scenes require at least 50 generated seconds: $6.50 on
OpenRouter, not $2.60. Manual reruns add output charges; a local timeout does not prove
that an upstream task stopped or was unbilled. Compare cost per **accepted** clip, not
only the cheapest listed second.

## Fit and required implementation changes

The current implementation lives in [src/video.rs](src/video.rs),
[src/openrouter.rs](src/openrouter.rs), [src/config.rs](src/config.rs), and
[src/ffmpeg.rs](src/ffmpeg.rs). The CLI already accepts arbitrary `--video-model` and
`--video-resolution` strings, but that is not verified H3 support:

1. **Duration:** unknown models receive Veo's discrete 4/6/8-second policy. H3 through
   OpenRouter needs its documented 5–15-second range; MiniMax direct/Replicate document
   4–15. Test minimum, fractional rounding, maximum, and reused-clip billing. Do not
   apply one family-wide range to all routes.
2. **Resolution and estimate:** tier defaults are 720p/1080p; OpenRouter H3 documents 2K.
   Resolve and validate the selected model's resolution before submission. H3 currently
   falls through to the unknown-model $0.05/sec estimate instead of $0.13/sec. Add the
   route-specific rate and test it together with billed seconds; leave tier defaults alone.
3. **Payload:** the OpenRouter client always sends `generate_audio: false`. Establish
   whether H3 accepts, ignores, or rejects it; MiniMax V2 has no documented audio-disable
   field. Do not advertise silent generation or retry invalid parameters as another job.
   Verify OpenRouter first-frame data-URI support and exact resolution spelling using
   its contract, not an assumed passthrough of MiniMax's API.
4. **References and aspect:** MiniMax V2 makes first/last-frame I2V mutually exclusive
   with reference-to-video, and derives I2V aspect from the supplied image. The existing
   client already separates the first-frame submission from the reference-only fallback;
   preserve that separation and test vertical input. Veo negative-prompt passthrough is
   already omitted for non-Veo model overrides.
5. **Lifecycle:** retain submit/poll/download, four-job concurrency, per-scene still
   fallback, and file reuse. Verify H3 status/error mapping, rate limits, and whether the
   current roughly ten-minute poll limit is sufficient. Avoid resubmitting an ambiguously
   accepted task; distinguish submission errors, terminal failure, and timeout.
6. **Assembly:** the current ffmpeg graph explicitly maps narration (and optional music),
   not audio from scene clips. Verify with a fixture containing native audio that it is
   excluded, and that 2K/768P clips are fitted and retimed correctly. No new audio feature
   is needed for this proposal.

For an OpenRouter implementation, update those existing owners and their tests rather
than introducing a general provider framework. Only extract a video-specific transport
boundary if a second backend is actually selected; keep text/image/TTS on OpenRouter and
use `REELMAESTRO_*` for any new credentials. Update README, CLI help, `.env.example`, and
CONTRIBUTING as applicable when runtime support lands. This research changes none of them
except a README link.

## Cloud GPU hosting and licensing

Renting GPUs is a different option from a managed H3 API, not a verified cheaper route.
No self-hosted throughput or cloud-GPU quote was measured in this investigation. Compare
GPU-hours per accepted clip plus idle capacity, model loading, storage, egress, maintenance,
and safety operations against the per-second API prices before considering deployment.
This investigation does not establish a native Bedrock, Vertex AI, or Azure H3 endpoint.

The [H3 community license](https://huggingface.co/MiniMaxAI/MiniMax-H3/blob/main/LICENSE)
is **not Apache-2.0**. It excludes the US, EU, UK, and South Korea from its grant; its use
restrictions also address outputs and hosted services. It requires separate prior written
authorization above $20M yearly commercial product/service revenue and includes commercial
UI attribution requirements. Do not infer deployment or output-use rights from “open weights”
or Reel Maestro's own license. Managed providers may have separate commercial arrangements;
confirm the applicable terms, permitted territories, output rights, data retention, and
processing regions before use. This is a procurement gate, not a legal conclusion that
every managed API is unavailable in those territories. No model weights or third-party
implementation are added by this proposal.

## Acceptance plan for a subsequent implementation

- Offline unit tests: H3 duration/resolution/pricing, serialized I2V/T2V payloads, no mixed
  frame/reference modes, no Veo passthrough, polling states/errors, timeout and reuse.
- Local ffmpeg smoke test: a synthetic clip with audio, portrait dimensions, non-default
  resolution and duration; verify output narration and timing. No model calls required.
- After explicit spending approval and rights confirmation: use three owned first-frame
  images covering portrait character motion, product/text fidelity, and environmental
  motion. Generate one 6-second clip per image per candidate route, with fixed prompts.
  OpenRouter H3 alone costs $2.34 in output charges; adding fal H3 768P costs $1.08,
  for $3.42 before other fees and reruns. Set an approved cap before starting; no automatic
  paid reruns for this experiment. These are different-resolution comparisons, not a
  controlled same-quality benchmark.
- Record actual charges, wall-clock latency, failures, aspect/duration, first-frame and
  identity fidelity, motion artifacts, unwanted text, and final narration isolation.
  Review the clips, then decide whether to ship an opt-in route. Do not change defaults
  based on vendor demos or advertised speed alone.
