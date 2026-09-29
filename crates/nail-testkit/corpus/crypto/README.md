# End-to-end test vectors

Keys, certificates and messages for the OpenPGP (`pgp/`, M2a WP31) and
S/MIME (`smime/`, M2a WP32) tests. They are **test material only**: every
secret key here is public knowledge and must never be used for real mail.

- Not demo mail: `demo_corpus()` / `load_mail_dir()` skip this directory;
  tests read files with `nail_testkit::crypto_fixture(CryptoFixtures::…, name)`.
- Generate once with the real tools (GnuPG, Thunderbird, OpenSSL) and check
  the results in; record in the directory's README how each file was made
  (command lines, tool versions) so it can be regenerated.
- Keep line endings as produced (`.gitattributes` marks the files binary
  where needed; signed MIME parts are byte-exact).
