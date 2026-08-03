"""PII guardrail, compiled to a Wasm component with componentize-py.

This is not a service: it exports `ymori:guardrail/pii` and is linked directly
into the Rust HTTP handler at build time. A CPython interpreter ends up running
inside the same component as compiled Rust and Go.
"""

import re

import wit_world.exports
from wit_world.exports.guard import Verdict

# Card-shaped runs of 13-19 digits, allowing spaces or hyphens as separators.
CARD_RE = re.compile(r"\b(?:\d[ -]?){13,19}\b")
EMAIL_RE = re.compile(r"\b[\w.+-]+@[\w-]+\.[\w.-]+\b")
# Japanese mobile/landline and generic international forms.
PHONE_RE = re.compile(r"(?:\+81[ -]?\d{1,4}|\b0\d{1,4})[ -]?\d{1,4}[ -]?\d{3,4}\b")
MYNUMBER_RE = re.compile(r"\b\d{4}[ -]?\d{4}[ -]?\d{4}\b")


def _luhn_ok(digits: str) -> bool:
    """Card numbers pass the Luhn checksum; random 16-digit runs usually do not."""
    total = 0
    for i, ch in enumerate(reversed(digits)):
        n = int(ch)
        if i % 2 == 1:
            n *= 2
            if n > 9:
                n -= 9
        total += n
    return total % 10 == 0


def _find_card(text: str) -> str | None:
    for match in CARD_RE.finditer(text):
        digits = re.sub(r"[ -]", "", match.group())
        if 13 <= len(digits) <= 19 and _luhn_ok(digits):
            return digits
    return None


class Guard(wit_world.exports.Guard):
    def check(self, text: str) -> Verdict:
        card = _find_card(text)
        if card is not None:
            masked = "*" * (len(card) - 4) + card[-4:]
            return Verdict(
                blocked=True,
                reason=f"Python guard: card number detected ({masked}, Luhn valid)",
            )

        if EMAIL_RE.search(text):
            return Verdict(blocked=True, reason="Python guard: email address detected")

        # Check My Number before the looser phone pattern so the reason is precise.
        if MYNUMBER_RE.search(text):
            return Verdict(
                blocked=True, reason="Python guard: 12-digit My Number detected"
            )

        if PHONE_RE.search(text):
            return Verdict(blocked=True, reason="Python guard: phone number detected")

        return Verdict(blocked=False, reason="")
