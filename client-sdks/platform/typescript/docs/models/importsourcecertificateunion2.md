# ImportSourceCertificateUnion2

Certificate publication or reference mode for Kubernetes public endpoints.


## Supported Types

### `models.ImportSourceCertificateTLSSecretRef2`

```typescript
const value: models.ImportSourceCertificateTLSSecretRef2 = {
  secretName: "<value>",
  mode: "tlsSecretRef",
};
```

### `models.ImportSourceCertificateManagedAcmImport2`

```typescript
const value: models.ImportSourceCertificateManagedAcmImport2 = {
  mode: "managedAcmImport",
};
```

### `models.ImportSourceCertificateAwsAcmArn2`

```typescript
const value: models.ImportSourceCertificateAwsAcmArn2 = {
  certificateArn: "<value>",
  mode: "awsAcmArn",
};
```

### `models.ImportSourceCertificateManagedTLSSecret2`

```typescript
const value: models.ImportSourceCertificateManagedTLSSecret2 = {
  mode: "managedTlsSecret",
  secretNameTemplate: "<value>",
};
```

### `models.ImportSourceCertificateNone2`

```typescript
const value: models.ImportSourceCertificateNone2 = {
  mode: "none",
};
```

