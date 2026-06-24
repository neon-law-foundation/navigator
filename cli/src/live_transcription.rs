use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use anyhow::{anyhow, bail, Context};
use reqwest::multipart::{Form, Part};
use serde::{Deserialize, Serialize};

pub struct CoverArgs {
    pub template: PathBuf,
    pub transcript: Option<PathBuf>,
    pub audio: Option<PathBuf>,
    pub model: String,
    pub api_base: String,
    pub api_key: Option<String>,
    pub pretty: bool,
}

#[derive(Debug, Deserialize)]
struct TemplateFrontmatter {
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    questionnaire: BTreeMap<String, BTreeMap<String, String>>,
}

#[derive(Debug, Deserialize)]
struct Records<T> {
    records: Vec<T>,
}

#[derive(Debug, Deserialize)]
struct QuestionSeed {
    code: String,
    prompt: String,
    #[serde(default)]
    question_type: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct Inquiry {
    code: String,
    prompt: String,
    answer_type: String,
    source: InquirySource,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum InquirySource {
    TemplateQuestion {
        template_code: String,
        question_code: String,
    },
}

#[derive(Debug, Clone, Serialize)]
struct TranscriptSegment {
    id: String,
    provider_sequence: usize,
    text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum CoverageStatus {
    LikelyAnswered,
    NeedsFollowUp,
}

#[derive(Debug, Clone, Serialize)]
struct CoverageFinding {
    inquiry_code: String,
    status: CoverageStatus,
    confidence: f32,
    proposed_answer: Option<String>,
    evidence_segment_ids: Vec<String>,
    follow_up_prompt: Option<String>,
}

#[derive(Debug, Serialize)]
struct CoverageOutput {
    template_code: String,
    transcript_source: TranscriptSource,
    transcript_text: String,
    inquiries: Vec<Inquiry>,
    transcript_segments: Vec<TranscriptSegment>,
    findings: Vec<CoverageFinding>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum TranscriptSource {
    Audio { path: String, model: String },
    TranscriptFile { path: String },
}

pub async fn cover(args: CoverArgs) -> anyhow::Result<()> {
    let template = load_template(&args.template)?;
    let questions = load_question_registry()?;
    let inquiries = normalize_inquiries(&template, &questions)?;

    let (transcript_text, transcript_source) = match (args.transcript, args.audio) {
        (Some(transcript), None) => (
            std::fs::read_to_string(&transcript)
                .with_context(|| format!("read transcript {}", transcript.display()))?,
            TranscriptSource::TranscriptFile {
                path: transcript.display().to_string(),
            },
        ),
        (None, Some(audio)) => {
            let api_key = args
                .api_key
                .as_deref()
                .ok_or_else(|| anyhow!("OPENAI_API_KEY or --api-key is required with --audio"))?;
            let text = transcribe_audio(&args.api_base, api_key, &args.model, &audio).await?;
            (
                text,
                TranscriptSource::Audio {
                    path: audio.display().to_string(),
                    model: args.model,
                },
            )
        }
        (None, None) => bail!("pass either --transcript or --audio"),
        (Some(_), Some(_)) => bail!("pass only one of --transcript or --audio"),
    };

    let transcript_segments = segment_transcript(&transcript_text);
    let findings = evaluate_coverage(&inquiries, &transcript_segments, &transcript_text);
    let output = CoverageOutput {
        template_code: template.code,
        transcript_source,
        transcript_text,
        inquiries,
        transcript_segments,
        findings,
    };
    if args.pretty {
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!("{}", serde_json::to_string(&output)?);
    }
    Ok(())
}

async fn transcribe_audio(
    api_base: &str,
    api_key: &str,
    model: &str,
    audio: &PathBuf,
) -> anyhow::Result<String> {
    let bytes = std::fs::read(audio).with_context(|| format!("read audio {}", audio.display()))?;
    let filename = audio
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("audio")
        .to_string();
    let form = Form::new()
        .part("file", Part::bytes(bytes).file_name(filename))
        .text("model", model.to_string())
        .text("response_format", "text");
    let url = format!("{}/audio/transcriptions", api_base.trim_end_matches('/'));
    let response = reqwest::Client::new()
        .post(url)
        .bearer_auth(api_key)
        .multipart(form)
        .send()
        .await
        .context("call OpenAI transcription endpoint")?;
    let status = response.status();
    let body = response
        .text()
        .await
        .context("read transcription response")?;
    if !status.is_success() {
        bail!("OpenAI transcription returned {status}: {body}");
    }
    Ok(body.trim().to_string())
}

struct LoadedTemplate {
    code: String,
    questionnaire: BTreeMap<String, BTreeMap<String, String>>,
}

fn load_template(path: &PathBuf) -> anyhow::Result<LoadedTemplate> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("read template {}", path.display()))?;
    let frontmatter = rules::frontmatter::extract(&raw)
        .ok_or_else(|| anyhow!("template {} has no frontmatter", path.display()))?;
    let parsed: TemplateFrontmatter = serde_yaml::from_str(frontmatter)
        .with_context(|| format!("parse template frontmatter {}", path.display()))?;
    let code = parsed.code.unwrap_or_else(|| {
        path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("untitled")
            .to_string()
    });
    Ok(LoadedTemplate {
        code,
        questionnaire: parsed.questionnaire,
    })
}

fn load_question_registry() -> anyhow::Result<BTreeMap<String, QuestionSeed>> {
    const QUESTION_YAML: &str = include_str!("../../store/seeds/Question.yaml");
    let parsed: Records<QuestionSeed> =
        serde_yaml::from_str(QUESTION_YAML).context("parse canonical Question.yaml")?;
    Ok(parsed
        .records
        .into_iter()
        .map(|q| (q.code.clone(), q))
        .collect())
}

fn normalize_inquiries(
    template: &LoadedTemplate,
    questions: &BTreeMap<String, QuestionSeed>,
) -> anyhow::Result<Vec<Inquiry>> {
    let mut current = "BEGIN".to_string();
    let mut seen = BTreeSet::new();
    let mut inquiries = Vec::new();
    while current != "END" {
        if !seen.insert(current.clone()) {
            bail!("questionnaire cycle at `{current}`");
        }
        let transitions = template
            .questionnaire
            .get(&current)
            .ok_or_else(|| anyhow!("questionnaire state `{current}` has no transitions"))?;
        let next = transitions
            .get("_")
            .or_else(|| transitions.values().next())
            .ok_or_else(|| anyhow!("questionnaire state `{current}` has no next state"))?;
        if next == "END" {
            break;
        }
        let Some(question) = questions.get(next) else {
            bail!("questionnaire references unknown question `{next}`");
        };
        inquiries.push(Inquiry {
            code: next.clone(),
            prompt: question.prompt.clone(),
            answer_type: question
                .question_type
                .clone()
                .unwrap_or_else(|| "string".to_string()),
            source: InquirySource::TemplateQuestion {
                template_code: template.code.clone(),
                question_code: next.clone(),
            },
        });
        current = next.clone();
    }
    Ok(inquiries)
}

fn segment_transcript(transcript: &str) -> Vec<TranscriptSegment> {
    let mut chunks: Vec<String> = transcript
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect();
    if chunks.len() <= 1 {
        chunks = sentence_chunks(transcript);
    }
    chunks
        .into_iter()
        .enumerate()
        .map(|(idx, text)| TranscriptSegment {
            id: format!("segment_{}", idx + 1),
            provider_sequence: idx + 1,
            text,
        })
        .collect()
}

fn sentence_chunks(transcript: &str) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut start = 0;
    for (idx, ch) in transcript.char_indices() {
        if matches!(ch, '.' | '?' | '!') {
            let end = idx + ch.len_utf8();
            let chunk = transcript[start..end].trim();
            if !chunk.is_empty() {
                chunks.push(chunk.to_string());
            }
            start = end;
        }
    }
    let tail = transcript[start..].trim();
    if !tail.is_empty() {
        chunks.push(tail.to_string());
    }
    if chunks.is_empty() && !transcript.trim().is_empty() {
        chunks.push(transcript.trim().to_string());
    }
    chunks
}

fn evaluate_coverage(
    inquiries: &[Inquiry],
    segments: &[TranscriptSegment],
    transcript: &str,
) -> Vec<CoverageFinding> {
    inquiries
        .iter()
        .map(|inquiry| {
            let (answer, evidence_segment_ids) =
                proposed_answer(&inquiry.code, transcript, segments);
            let status = if answer.is_some() {
                CoverageStatus::LikelyAnswered
            } else {
                CoverageStatus::NeedsFollowUp
            };
            CoverageFinding {
                inquiry_code: inquiry.code.clone(),
                status,
                confidence: if status == CoverageStatus::LikelyAnswered {
                    0.74
                } else {
                    0.2
                },
                proposed_answer: answer,
                evidence_segment_ids,
                follow_up_prompt: (status == CoverageStatus::NeedsFollowUp)
                    .then(|| inquiry.prompt.clone()),
            }
        })
        .collect()
}

fn proposed_answer(
    code: &str,
    transcript: &str,
    segments: &[TranscriptSegment],
) -> (Option<String>, Vec<String>) {
    let lower = transcript.to_lowercase();
    if code == "recording_consent" && lower.contains("consent") {
        return (
            Some("Yes".to_string()),
            evidence_segments(segments, &["consent"], None),
        );
    }
    for label in labels_for(code) {
        if let Some(value) = value_after_label(&lower, transcript, label) {
            return (
                Some(value.clone()),
                evidence_segments(segments, &[*label], Some(&value)),
            );
        }
    }
    (None, Vec::new())
}

fn labels_for(code: &str) -> &'static [&'static str] {
    match code {
        "testator_name" => &["testator", "full legal name", "my name is"],
        "executor_name" => &["executor"],
        "successor_trustee" => &["successor trustee", "trustee"],
        "guardian_for_minors" => &["guardian"],
        "residuary_beneficiary" => &["residuary beneficiary", "beneficiary"],
        "healthcare_agent" => &["health-care agent", "healthcare agent", "health care agent"],
        "financial_agent" => &["financial agent"],
        _ => &[],
    }
}

fn value_after_label(lower: &str, transcript: &str, label: &str) -> Option<String> {
    let at = lower.find(label)?;
    let mut start = at + label.len();
    let lower_tail = lower[start..].trim_start();
    let trimmed = lower[start..].len() - lower_tail.len();
    start += trimmed;
    for prefix in [
        ":",
        "-",
        "is going to be",
        "will be",
        "would be",
        "should be",
        "is",
    ] {
        let tail = lower[start..].trim_start();
        if let Some(stripped) = tail.strip_prefix(prefix) {
            start = lower.len() - stripped.len();
            break;
        }
    }
    let tail = transcript[start..].trim_start_matches([' ', ':', '-']);
    let end = tail.find(['.', ';', '\n']).unwrap_or(tail.len());
    let value = tail[..end].trim().trim_matches('"');
    (!value.is_empty()).then(|| value.to_string())
}

fn evidence_segments(
    segments: &[TranscriptSegment],
    labels: &[&str],
    answer: Option<&str>,
) -> Vec<String> {
    let answer_lower = answer.map(str::to_lowercase);
    segments
        .iter()
        .filter(|segment| {
            let lower = segment.text.to_lowercase();
            labels.iter().any(|label| lower.contains(label))
                || answer_lower
                    .as_ref()
                    .is_some_and(|value| lower.contains(value))
        })
        .map(|segment| segment.id.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_estate_questionnaire_into_inquiries() {
        let template = LoadedTemplate {
            code: "onboarding__estate".to_string(),
            questionnaire: serde_yaml::from_str(
                r"
BEGIN:
  _: recording_consent
recording_consent:
  _: testator_name
testator_name:
  _: END
END: {}
",
            )
            .unwrap(),
        };
        let questions = load_question_registry().unwrap();

        let inquiries = normalize_inquiries(&template, &questions).unwrap();

        assert_eq!(
            inquiries
                .iter()
                .map(|inquiry| inquiry.code.as_str())
                .collect::<Vec<_>>(),
            vec!["recording_consent", "testator_name"]
        );
        assert_eq!(inquiries[1].prompt, "What is your full legal name?");
    }

    #[test]
    fn coverage_finds_answers_and_evidence_segments() {
        let inquiries = vec![
            Inquiry {
                code: "recording_consent".to_string(),
                prompt: "Do you consent to recording this sitting?".to_string(),
                answer_type: "yes_no".to_string(),
                source: InquirySource::TemplateQuestion {
                    template_code: "onboarding__estate".to_string(),
                    question_code: "recording_consent".to_string(),
                },
            },
            Inquiry {
                code: "executor_name".to_string(),
                prompt: "Who is the executor of your will?".to_string(),
                answer_type: "string".to_string(),
                source: InquirySource::TemplateQuestion {
                    template_code: "onboarding__estate".to_string(),
                    question_code: "executor_name".to_string(),
                },
            },
            Inquiry {
                code: "financial_agent".to_string(),
                prompt: "Who is your financial agent under a durable power of attorney?"
                    .to_string(),
                answer_type: "string".to_string(),
                source: InquirySource::TemplateQuestion {
                    template_code: "onboarding__estate".to_string(),
                    question_code: "financial_agent".to_string(),
                },
            },
        ];
        let transcript = "I consent to recording this sitting.\nThe executor will be Jamie Rivera.";
        let segments = segment_transcript(transcript);

        let findings = evaluate_coverage(&inquiries, &segments, transcript);

        assert_eq!(findings[0].status, CoverageStatus::LikelyAnswered);
        assert_eq!(findings[1].proposed_answer.as_deref(), Some("Jamie Rivera"));
        assert_eq!(findings[1].evidence_segment_ids, vec!["segment_2"]);
        assert_eq!(findings[2].status, CoverageStatus::NeedsFollowUp);
        assert!(findings[2].follow_up_prompt.is_some());
    }
}
