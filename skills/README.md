# VanguardCV Skills Engine & Manifest Format

VanguardCV supports modular, user-extensible AI skills modeled after the `oh-my-pi` (`omp`) architecture. Skills supply specialized domain rules, constraints, and system prompt injections to the Antigravity agent loop when generating tailored Child Sub-Variations or running inline rewrites.

---

## 1. Directory Structure & Discovery

Skills are discovered and loaded from two locations:
1. **Bundled Default Skills:** Shipped with VanguardCV in `skills/*.toml`.
2. **User Skills:** Located at `~/.config/vanguard-cv/skills/` (or platform equivalent).
   - Created automatically on first run.
   - User skills can override bundled skills with the same `id` or introduce new custom skills.
   - Monitored in real time via filesystem events (`notify`) for instant hot reload.

---

## 2. File Formats

Skills can be defined as pure **TOML files** (`.toml`) or **Markdown files with TOML frontmatter** (`.md`).

### A. Pure TOML Manifest (`.toml`)

```toml
id = "ats_classic_optimizer"
name = "ATS Classic Optimizer"
category = "Formatting & ATS"
version = "1.0.0"
description = "Keeps the CV parseable by classic ATS engines and fronts grounded keywords."
default_enabled = true

[rules]
enforce_action_verbs = true
prefer_quantified_metrics = true
max_lines_per_bullet = 2
strip_first_person_pronouns = true

[prompt_injection]
system_directives = """
- Begin every experience bullet with a strong past-tense action verb (or present-tense if role is (Currently)).
- Align technical terminology in SKILLS AND OTHERS and PROFESSIONAL EXPERIENCE with the exact phrasing in the Job Description.
- Never invent companies, degrees, or technologies not grounded in the base CV.
- Format inline skill categories concisely with ' - ' or ', ' separators.
- Eliminate first-person pronouns (I, my, we).
"""
```

### B. Markdown with Frontmatter (`.md`)

VanguardCV supports TOML frontmatter delimited by either `+++` (primary) or `---` (fallback):

````markdown
+++
id = "security_role_enhancer"
name = "Offensive Security Enhancer"
category = "Domain Expertise"
version = "1.0.0"
description = "Frames achievements using industry-standard penetration testing and Red Team terminology."
default_enabled = false

[rules]
framework = "mitre_attck"
target_depth = "deep"
+++

# Directives

- Map vulnerability discovery statements to standard CVE/CWE framing where appropriate.
- Highlight methodology phases (Reconnaissance, Exploitation, Lateral Movement, Reporting).
- Emphasize remediation guidance delivered to engineering teams.
````

When using Markdown, directives can either be specified inside `[prompt_injection] system_directives = "..."` in the frontmatter, in the Markdown body itself, or combined.

---

## 3. Schema Fields

| Field | Type | Required | Description |
| :--- | :--- | :--- | :--- |
| `id` | `String` | Yes | Unique identifier (e.g., `ats_classic_optimizer`, `xyz_bullet_enhancer`). |
| `name` | `String` | Yes | Human-readable name displayed in the UI chips and wizard. |
| `category` | `String` | No | Category grouping (e.g. `Formatting & ATS`, `Writing Quality`, `Targeting`). |
| `version` | `String` | No | Semantic version string (e.g. `1.0.0`). |
| `description` | `String` | No | Tooltip / description summary explaining what the skill accomplishes. |
| `default_enabled` | `bool` | No | Whether the skill is pre-selected when opening the AI Wizard (default: `false`). |
| `rules` | `Table` | No | Arbitrary key-value configuration table inspected by rules engines. |
| `prompt_injection` | `Table` | No | Contains `system_directives` injected into the Antigravity agent system prompt. |

---

## 4. Default Bundled Skills

VanguardCV bundles four core Phase 3 skills in `skills/`:

1. **`ats_classic_optimizer.toml`**: Front-loads grounded keywords, enforces strong action verbs, eliminates first-person pronouns, and ensures classic ATS parsing compatibility.
2. **`xyz_bullet_enhancer.toml`**: Restructures bullet points using Google's XYZ formula (*Accomplished [X], as measured by [Y], by doing [Z]*). Strictly forbids inventing ungrounded metrics.
3. **`keyword_aligner.toml`**: Evaluates the job description against candidate experience, aligns terminology with target JD phrasing, and assists in scoring ATS match percentage.
4. **`page_compressor.toml`**: Condenses bullet points and trims verbosity to fit CV within page budget without losing technical impact or keywords.

---

## 5. Security & Grounding Policy

All skills operate under VanguardCV's **Zero-Trust PII Air-Gap** and **Anti-Hallucination Directives**:
- The AI never receives PII (name, email, phone, location, links); all payloads are scrubbed prior to network transmission.
- Directives must **never instruct the model to fabricate** companies, universities, degrees, dates, metrics, or technologies not grounded in the candidate's base CV.
