# KubernetesCertificateMode

Certificate publication or reference mode for Kubernetes public endpoints.


## Supported Types

### `models.KubernetesCertificateModeManagedAcmImport`

```typescript
const value: models.KubernetesCertificateModeManagedAcmImport = {
  mode: "managedAcmImport",
};
```

### `models.KubernetesCertificateModeAwsAcmArn`

```typescript
const value: models.KubernetesCertificateModeAwsAcmArn = {
  certificateArn: "<value>",
  mode: "awsAcmArn",
};
```

### `models.KubernetesCertificateModeManagedTLSSecret`

```typescript
const value: models.KubernetesCertificateModeManagedTLSSecret = {
  mode: "managedTlsSecret",
  secretNameTemplate: "<value>",
};
```

### `models.KubernetesCertificateModeTLSSecretRef`

```typescript
const value: models.KubernetesCertificateModeTLSSecretRef = {
  secretName: "<value>",
  mode: "tlsSecretRef",
};
```

### `models.KubernetesCertificateModeNone`

```typescript
const value: models.KubernetesCertificateModeNone = {
  mode: "none",
};
```

