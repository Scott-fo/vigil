use std::cmp::Ordering;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SegmentSortKey {
    lower_value: String,
    tokens: Vec<NaturalToken>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum NaturalToken {
    Number(u64),
    Text(String),
}

#[inline]
pub(super) fn create_segment_sort_key(value: &str) -> SegmentSortKey {
    let lower_value = value.to_lowercase();
    SegmentSortKey {
        tokens: split_into_natural_tokens(&lower_value),
        lower_value,
    }
}

#[inline]
pub(super) fn compare_segment_sort_keys(
    left_key: &SegmentSortKey,
    right_key: &SegmentSortKey,
) -> Ordering {
    if let ([NaturalToken::Text(left)], [NaturalToken::Text(right)]) =
        (left_key.tokens.as_slice(), right_key.tokens.as_slice())
    {
        return left.cmp(right);
    }

    let token_order = compare_natural_tokens(&left_key.tokens, &right_key.tokens);
    if token_order != Ordering::Equal {
        return token_order;
    }
    left_key.lower_value.cmp(&right_key.lower_value)
}

#[inline]
fn split_into_natural_tokens(value: &str) -> Vec<NaturalToken> {
    let mut tokens = Vec::new();
    let bytes = value.as_bytes();
    let mut token_start = 0usize;
    let mut index = 0usize;

    while index < bytes.len() {
        while index < bytes.len() && !bytes[index].is_ascii_digit() {
            index += 1;
        }
        if index >= bytes.len() {
            break;
        }

        if index > token_start {
            tokens.push(NaturalToken::Text(value[token_start..index].to_string()));
        }

        let mut number = 0u64;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            number = number
                .saturating_mul(10)
                .saturating_add((bytes[index] - b'0') as u64);
            index += 1;
        }
        tokens.push(NaturalToken::Number(number));
        token_start = index;
    }

    if token_start < value.len() || tokens.is_empty() {
        tokens.push(NaturalToken::Text(value[token_start..].to_string()));
    }
    tokens
}

#[inline]
fn compare_natural_tokens(left: &[NaturalToken], right: &[NaturalToken]) -> Ordering {
    for (left_token, right_token) in left.iter().zip(right.iter()) {
        let order = match (left_token, right_token) {
            (NaturalToken::Number(left), NaturalToken::Number(right)) => left.cmp(right),
            (NaturalToken::Text(left), NaturalToken::Text(right)) => left.cmp(right),
            (NaturalToken::Number(left), NaturalToken::Text(right)) => left.to_string().cmp(right),
            (NaturalToken::Text(left), NaturalToken::Number(right)) => left.cmp(&right.to_string()),
        };
        if order != Ordering::Equal {
            return order;
        }
    }

    left.len().cmp(&right.len())
}
