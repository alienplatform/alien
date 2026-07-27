# Embedded Postgres root CAs

These files contain only self-signed root certificates. Server and intermediate
certificates are intentionally excluded because AWS and Azure rotate them routinely.

- `aws-rds-global-roots.pem` is the official [Amazon RDS global bundle][aws-bundle].
  Despite its size, every entry is a region-specific, self-signed `CA:TRUE` root for
  one of RDS's supported CA algorithms. It was refreshed on 2026-07-27.
- `azure-postgres-roots.pem` contains the roots [Microsoft currently recommends][azure-docs]
  for Azure Database for PostgreSQL: DigiCert Global Root G2, Microsoft RSA Root
  Certificate Authority 2017, and the DigiCert Global Root CA retained for China
  regions and rotation extensions. The certificate sources are Microsoft's
  [RSA 2017 root][microsoft-rsa-root], DigiCert's [Global Root G2][digicert-g2],
  and DigiCert's [Global Root CA][digicert-g1]. It was refreshed on 2026-07-27.

Shared-provider root rotation is handled by updating these files and releasing
`alien-bindings`. Cloud SQL is different: its CA is instance-specific and therefore
travels in the public binding.

Before an update, inspect every certificate with OpenSSL and verify:

1. `subject` equals `issuer`;
2. basic constraints contain `CA:TRUE`;
3. the provider documentation still lists the root.

Current SHA-256 checksums:

```text
e5bb2084ccf45087bda1c9bffdea0eb15ee67f0b91646106e466714f9de3c7e3  aws-rds-global-roots.pem
0d005d74af16cc8330cbaa19feb1308edede7b73978782e7a7a905541f5ca5fc  azure-postgres-roots.pem
```

[aws-bundle]: https://truststore.pki.rds.amazonaws.com/global/global-bundle.pem
[azure-docs]: https://learn.microsoft.com/azure/postgresql/security/security-tls-how-to-connect
[microsoft-rsa-root]: https://www.microsoft.com/pkiops/certs/Microsoft%20RSA%20Root%20Certificate%20Authority%202017.crt
[digicert-g2]: https://cacerts.digicert.com/DigiCertGlobalRootG2.crt.pem
[digicert-g1]: https://cacerts.digicert.com/DigiCertGlobalRootCA.crt.pem
