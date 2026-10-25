// Run: swift scripts/verify-connect-crypto.swift crates/core/src/connect/testdata/vault-v1.json
import Foundation
import CryptoKit

func decode(_ value: String) -> Data {
    let base = value.replacingOccurrences(of: "-", with: "+").replacingOccurrences(of: "_", with: "/")
    return Data(base64Encoded: base + String(repeating: "=", count: (4 - base.count % 4) % 4))!
}
let data = try Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[1]))
let fixture = try JSONSerialization.jsonObject(with: data) as! [String: Any]
let bundle = fixture["keys"] as! [String: Any]
let keys = bundle["keys"] as! [String: String]
let envelope = fixture["envelope"] as! [String: Any]
let key = SymmetricKey(data: decode(keys["1"]!))
let aad: [Any] = ["discophile-connect-vault", 1, envelope["instanceId"]!, envelope["vaultId"]!,
                  envelope["epoch"]!, envelope["senderId"]!, envelope["objectId"]!, envelope["revision"]!]
let context = try JSONSerialization.data(withJSONObject: aad, options: [.withoutEscapingSlashes])
let encrypted = decode(envelope["ciphertext"] as! String)
let nonce = try AES.GCM.Nonce(data: decode(envelope["nonce"] as! String))
let box = try AES.GCM.SealedBox(nonce: nonce, ciphertext: encrypted.dropLast(16), tag: encrypted.suffix(16))
let plain = try AES.GCM.open(box, using: key, authenticating: context)
let actual = try JSONSerialization.jsonObject(with: plain) as! NSDictionary
let expected = fixture["plaintext"] as! NSDictionary
precondition(actual == expected)
let encoded = try AES.GCM.seal(plain, using: key, nonce: nonce, authenticating: context)
precondition(encoded.ciphertext + encoded.tag == encrypted)
do {
    _ = try AES.GCM.open(box, using: key, authenticating: Data("wrong-context".utf8))
    fatalError("Unauthenticated envelope accepted")
} catch { print("PASS: CryptoKit decrypts the Rust/Node fixture, reproduces its ciphertext, and rejects substitution.") }
