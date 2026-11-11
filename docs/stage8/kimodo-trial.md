# Kimodo trial (Stage 8, throwaway): stopped before any clip

Date: 2026-10-21. Branch `s8-kimodo-trial` (not for merge). Nothing here reaches any game's download.

Goal: try NVIDIA Kimodo (https://github.com/nv-tlabs/kimodo, code Apache-2.0, SOMA models under the
NVIDIA Open Model License) as an offline source of idle / walk / run / wave clips for human rigs.
Chris stopped the trial at the first hard blocker (the Llama 3 gate, below). No clips, BVH, retarget,
`cargo xtask convert` or rig check were run.

## What was done
- Checked the PC: RTX 4060 Ti, **8 GB VRAM** (2.3 GB already in use), 64 GB RAM (57% in use, 8.2 GB
  headroom under the 70% budget), 690 GB free disk.
- Cloned Kimodo into `..\tools\kimodo\src` (shallow, untrusted, own folder).
- Made a uv venv (Python 3.12) at `..\tools\kimodo\venv` and installed PyTorch 2.11.0+cu128
  (CUDA works). System Python untouched.
- `pip install -e ".[soma]"` **failed**: building the editable package needs **CMake**, which is not on
  the PATH (`RuntimeError: CMake must be installed to build this package`). Not pursued after the stop.

## Blockers found
1. **Gated Llama 3 text encoder.** Kimodo's text encoder is LLM2Vec on top of
   `meta-llama/Meta-Llama-3-8B-Instruct` (the McGill-NLP repos hold only LoRA adapters). That model is
   gated on Hugging Face: you must accept Meta's Llama 3 Community Licence on its page. Chris's
   Hugging Face token works and is allowed to read gated repos, but the account is not on the access
   list (HTTP 403). The research report missed this, and it adds a second licence (Meta Llama 3) to the
   stack. Chris chose to stop rather than request access or use a re-shared copy.
2. **CMake** is needed for the editable install on Windows (see above).
3. **Memory on this PC.** The encoder is an 8B model in bfloat16, about 16 GB. Full-GPU use needs about
   17 GB VRAM; this card has 8 GB. `TEXT_ENCODER_DEVICE=cpu` brings VRAM under 3 GB but puts about 16 GB
   in system RAM, roughly twice today's 8.2 GB headroom. Workable routes if retried: encode the four
   prompts once with disk offload (accelerate `max_memory`), cache the embeddings, then pass a cached
   encoder to `load_model(text_encoder=...)`; or free RAM first. Set `TEXT_ENCODER_MODE=local` too: the
   default `auto` first tries a text-encoder server on 127.0.0.1:9550.

## Useful facts for a retry
- CLI: `kimodo_gen "<prompt>" --duration <s> --model Kimodo-SOMA-RP-v1.2`; BVH comes from
  `kimodo_convert` (`kimodo` -> `soma-bvh`, SOMA 77 joints under a `Root` wrapper). Use SOMA models only,
  never SMPL-X (research-only licence).
- Retarget target in the repo: `art-source/meshy/frontier-marshal/frontier-marshal.rig.blend`, with the
  pinned Blender at `.evani/tools/blender-5.2.2-windows-x64`.
- Idea to copy into our own tools: keyframe constraints (pin the start and end poses and let the model
  fill in between), which give clean loop seams for idle and walk cycles.

## Disk left on this PC (nothing deleted)
- `..\tools\kimodo\venv`: 4.2 GB (mostly PyTorch with CUDA)
- `..\tools\kimodo\src`: 122 MB
- No model weights were downloaded. The saved copy of the Hugging Face token was removed.

## Verdict
Not adopted for now. It is blocked on the Llama 3 licence step, CMake and RAM. Retry only if Chris
accepts Meta's licence; Mixamo or hand-keyed clips stay the route for human motion until then.
