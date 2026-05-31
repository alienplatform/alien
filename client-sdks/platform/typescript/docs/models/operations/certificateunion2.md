# CertificateUnion2

Certificate publication or reference mode for Kubernetes public endpoints.


## Supported Types

### `operations.CertificateTLSSecretRef2`

```typescript
const value: operations.CertificateTLSSecretRef2 = {
  secretName: "<value>",
  mode: "tlsSecretRef",
};
```

### `operations.CertificateManagedAcmImport2`

```typescript
const value: operations.CertificateManagedAcmImport2 = {
  mode: "managedAcmImport",
};
```

### `operations.CertificateAwsAcmArn2`

```typescript
const value: operations.CertificateAwsAcmArn2 = {
  certificateArn: "<value>",
  mode: "awsAcmArn",
};
```

### `operations.CertificateManagedTLSSecret2`

```typescript
const value: operations.CertificateManagedTLSSecret2 = {
  mode: "managedTlsSecret",
  secretNameTemplate: "<value>",
};
```

### `operations.CertificateNone2`

```typescript
const value: operations.CertificateNone2 = {
  mode: "none",
};
```

