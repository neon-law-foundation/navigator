use maud::{html, Markup};

pub struct TestimonialCard<'a> {
    pub quote: &'a str,
    pub attribution: &'a str,
    pub detail: Option<&'a str>,
    pub profile_image_url: Option<&'a str>,
    pub product_label: Option<&'a str>,
}

#[must_use]
pub fn testimonial_section(heading: &str, lead: &str, cards: &[TestimonialCard<'_>]) -> Markup {
    if cards.is_empty() {
        return html! {};
    }
    html! {
        section."mb-5"."testimonial-section" {
            div."d-flex"."flex-column"."flex-lg-row"."justify-content-between"."gap-3"."align-items-lg-end"."mb-4" {
                div {
                    h2."h3"."mb-2" { (heading) }
                    p."text-body-secondary"."mb-0" { (lead) }
                }
            }
            div."row"."row-cols-1"."row-cols-md-2"."g-4" {
                @for card in cards {
                    div."col" {
                        article."card"."h-100"."border-0"."shadow-sm"."testimonial-card" {
                            div."card-body"."d-flex"."flex-column"."gap-3" {
                                @if let Some(label) = card.product_label {
                                    p."text-uppercase"."fw-semibold"."text-primary"."small"."mb-0" {
                                        (label)
                                    }
                                }
                                blockquote."mb-0"."fs-5" {
                                    p."mb-0" { "“" (card.quote) "”" }
                                }
                                div."d-flex"."align-items-center"."gap-3"."mt-auto" {
                                    @if let Some(url) = card.profile_image_url {
                                        img."rounded-circle"."object-fit-cover"
                                            src=(url)
                                            alt=(format!("{} profile image", card.attribution))
                                            width="56"
                                            height="56";
                                    } @else {
                                        div."rounded-circle"."bg-primary-subtle"."text-primary"."fw-bold"."d-flex"."align-items-center"."justify-content-center"
                                            style="width: 56px; height: 56px;"
                                            aria-hidden="true" {
                                            (initials(card.attribution))
                                        }
                                    }
                                    div {
                                        p."fw-semibold"."mb-0" { (card.attribution) }
                                        @if let Some(detail) = card.detail {
                                            p."small"."text-body-secondary"."mb-0" { (detail) }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn initials(name: &str) -> String {
    let mut out = String::new();
    for part in name.split_whitespace().take(2) {
        if let Some(ch) = part.chars().next() {
            out.push(ch.to_ascii_uppercase());
        }
    }
    if out.is_empty() {
        out.push('N');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{testimonial_section, TestimonialCard};

    #[test]
    fn empty_section_renders_nothing() {
        assert_eq!(testimonial_section("Proof", "Lead", &[]).into_string(), "");
    }

    #[test]
    fn card_renders_quote_attribution_and_image() {
        let cards = [TestimonialCard {
            quote: "They moved quickly and explained every step.",
            attribution: "A. Client",
            detail: Some("Nexus matter"),
            profile_image_url: Some("/images/a-client.webp"),
            product_label: Some("Nexus"),
        }];
        let html = testimonial_section("Proof", "Lead", &cards).into_string();
        assert!(html.contains("They moved quickly and explained every step."));
        assert!(html.contains("A. Client"));
        assert!(html.contains("Nexus matter"));
        assert!(html.contains("src=\"/images/a-client.webp\""));
        assert!(html.contains("Nexus"));
    }
}
