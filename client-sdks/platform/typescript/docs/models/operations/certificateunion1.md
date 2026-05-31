# CertificateUnion1

Certificate publication or reference mode for Kubernetes public endpoints.


## Supported Types

### `operations.CertificateTLSSecretRef1`

```typescript
const value: operations.CertificateTLSSecretRef1 = {
  secretName: "<value>",
  mode: "tlsSecretRef",
};
```

### `operations.CertificateManagedAcmImport1`

```typescript
const value: operations.CertificateManagedAcmImport1 = {
  mode: "managedAcmImport",
};
```

### `operations.CertificateAwsAcmArn1`

```typescript
const value: operations.CertificateAwsAcmArn1 = {
  certificateArn: "<value>",
  mode: "awsAcmArn",
};
```

### `operations.CertificateManagedTLSSecret1`

```typescript
const value: operations.CertificateManagedTLSSecret1 = {
  mode: "managedTlsSecret",
  secretNameTemplate: "<value>",
};
```

### `operations.CertificateNone1`

```typescript
const value: operations.CertificateNone1 = {
  mode: "none",
};
```

