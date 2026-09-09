package main

import (
	"net/url"
	"testing"
)

func TestSignatureCompatibleWithReceiver(t *testing.T) {
	got := sign("secret", "POST", "/weighing-data-sync/put", "", "1787846400", "nonce", []byte(`{"records":[]}`))
	const want = "03e9c1d9d09c6b464137c4e3a005a1eb68b18caae2507825c6549e517942a4d6"
	if got != want {
		t.Fatalf("signature = %s", got)
	}
}
func TestCanonicalQuery(t *testing.T) {
	values := url.Values{"b": {"2", "1"}, "signature": {"skip"}, "a": {"x y"}}
	if got := canonicalQuery(values); got != "a=x+y&b=1&b=2" {
		t.Fatalf("query = %s", got)
	}
}
func TestAcknowledged(t *testing.T) {
	records := []sourceRecord{{pk: "1", serial: "a"}, {pk: "2", serial: "b"}}
	got := acknowledged(records, uploadResponse{Accepted: []string{"b"}})
	if len(got) != 1 || got[0].pk != "2" {
		t.Fatalf("unexpected: %#v", got)
	}
}
