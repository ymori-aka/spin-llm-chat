// Prompt-injection guardrail, compiled to a Wasm component with TinyGo.
//
// This is not a service: it exports `ymori:guardrail/injection` and is linked
// directly into the Rust HTTP handler at build time.
package main

import (
	"strings"

	"github.com/ymori-aka/spin-llm-chat/guards/injection-go/internal/ymori/injection/guard"
)

// Substring matching rather than regexp: it keeps the TinyGo binary small and
// the behaviour obvious when demoing which phrase tripped the guard.
var patterns = []struct {
	needle string
	reason string
}{
	{"ignore all previous", "instruction override attempt"},
	{"ignore previous instructions", "instruction override attempt"},
	{"disregard previous", "instruction override attempt"},
	{"disregard all prior", "instruction override attempt"},
	{"your new instructions", "instruction override attempt"},
	{"override instructions", "instruction override attempt"},
	{"これまでの指示を無視", "instruction override attempt (ja)"},
	{"前の指示を無視", "instruction override attempt (ja)"},

	{"do anything now", "jailbreak vocabulary"},
	{"developer mode", "jailbreak vocabulary"},
	{"pretend to be", "persona hijack attempt"},
	{"act as if you", "persona hijack attempt"},
	{"roleplay as", "persona hijack attempt"},
	{"になりきって", "persona hijack attempt (ja)"},

	{"system prompt", "system prompt exfiltration attempt"},
	{"reveal your instructions", "system prompt exfiltration attempt"},
	{"print your instructions", "system prompt exfiltration attempt"},
	{"システムプロンプト", "system prompt exfiltration attempt (ja)"},

	{"[inst]", "model delimiter injection"},
	{"<|system|>", "model delimiter injection"},
	{"<|im_start|>", "model delimiter injection"},
	{"### system", "model delimiter injection"},
}

func check(text string) guard.Verdict {
	lowered := strings.ToLower(text)
	for _, p := range patterns {
		if strings.Contains(lowered, p.needle) {
			return guard.Verdict{
				Blocked: true,
				Reason:  "Go guard: " + p.reason + " (\"" + p.needle + "\")",
			}
		}
	}
	return guard.Verdict{Blocked: false, Reason: ""}
}

func init() {
	guard.Exports.Check = check
}

// Required by TinyGo, but never called: the component has no command entrypoint.
func main() {}
