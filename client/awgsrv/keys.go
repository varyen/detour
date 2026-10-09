package main

import (
	"crypto/ecdh"
	"crypto/rand"
	"encoding/base64"
	"fmt"
	"io"
	"log"
	"os"
	"strings"
)

// keys — то же, что `awg genkey | pubkey | genpsk`: ключи Curve25519 в base64.
func keys(cmd string) {
	switch cmd {
	case "genkey", "genpsk":
		var k [32]byte
		if _, err := rand.Read(k[:]); err != nil {
			log.Fatal(err)
		}
		if cmd == "genkey" {
			k[0] &= 248
			k[31] = (k[31] & 127) | 64
		}
		fmt.Println(base64.StdEncoding.EncodeToString(k[:]))
	case "pubkey":
		in, err := io.ReadAll(os.Stdin)
		if err != nil {
			log.Fatal(err)
		}
		raw, err := base64.StdEncoding.DecodeString(strings.TrimSpace(string(in)))
		if err != nil || len(raw) != 32 {
			log.Fatal("pubkey: ждём приватный ключ base64 на stdin")
		}
		priv, err := ecdh.X25519().NewPrivateKey(raw)
		if err != nil {
			log.Fatal(err)
		}
		fmt.Println(base64.StdEncoding.EncodeToString(priv.PublicKey().Bytes()))
	}
}
