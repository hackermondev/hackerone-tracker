static HIGH: i32 = 50;
static MEDIUM: i32 = 25;
static LOW: i32 = 15;
static TRIAGED: i32 = 7;
static NOT_APPLICABLE: i32 = -2;
static SPAM: i32 = -10;

#[derive(Debug)]
pub struct ReputationBreakdown {
    high_bounty: i32,
    medium_bounty: i32,
    low_bounty: i32,
    triaged: i32,
    not_applicable: i32,
    spam: i32,
}

impl ToString for ReputationBreakdown {
    fn to_string(&self) -> String {
        let mut parts = Vec::new();

        macro_rules! add_part {
            ($condition:expr, $text:expr, $count:expr) => {
                if $condition > 0 {
                    parts.push(if $count < 2 {
                        $text.to_string()
                    } else {
                        format!("{}({})", $text, $count)
                    });
                }
            };
        }

        add_part!(self.high_bounty, "High", self.high_bounty);
        add_part!(self.medium_bounty, "Medium", self.medium_bounty);
        add_part!(self.low_bounty, "Low", self.low_bounty);
        add_part!(self.triaged, "Triage", self.triaged);
        add_part!(self.not_applicable, "N/A", self.not_applicable);
        add_part!(self.spam, "Spam", self.spam);

        parts.join(", ")
    }
}

pub fn calculate_rep_breakdown(mut rep_points: i32) -> ReputationBreakdown {
    let mut breakdown = ReputationBreakdown {
        high_bounty: 0,
        medium_bounty: 0,
        low_bounty: 0,
        triaged: 0,
        not_applicable: 0,
        spam: 0,
    };

    let thresholds = [HIGH, MEDIUM, LOW, TRIAGED, NOT_APPLICABLE, SPAM];
    let mut index = 0;

    while index < thresholds.len() {
        let is_over = {
            if thresholds[index] > 0 {
                rep_points >= thresholds[index]
            } else {
                rep_points <= thresholds[index]
            }
        };

        if is_over {
            let count = rep_points / thresholds[index];
            match index {
                0 => breakdown.high_bounty = count,
                1 => breakdown.medium_bounty = count,
                2 => breakdown.low_bounty = count,
                3 => breakdown.triaged = count,
                4 => breakdown.not_applicable = count,
                5 => breakdown.spam = count,
                _ => {}
            }

            rep_points -= count * thresholds[index];
        }

        index += 1;
    }

    breakdown
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple() {
        let a = calculate_rep_breakdown(7);
        let b = calculate_rep_breakdown(7 + 7);

        assert!(a.to_string() == "Triage");
        assert!(b.to_string() == "Triage(2)");
    }

    #[test]
    fn negative() {
        let a = calculate_rep_breakdown(-2);
        let b = calculate_rep_breakdown(-2 + -2);

        assert!(a.to_string() == "N/A");
        assert!(b.to_string() == "N/A(2)");
    }

    #[test]
    fn none() {
        let a = calculate_rep_breakdown(0);
        let b = calculate_rep_breakdown(3);

        assert!(a.to_string() == "");
        assert!(b.to_string() == "");
    }
}
