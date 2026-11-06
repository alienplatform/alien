# KubernetesExposureCustomBaseDomainCertificateUnion

Certificate publication or reference mode for Kubernetes public endpoints.


## Supported Types

### `models.KubernetesExposureCustomBaseDomainCertificateTLSSecretRef`

```typescript
const value: models.KubernetesExposureCustomBaseDomainCertificateTLSSecretRef =
  {
    secretName: "<value>",
    mode: "tlsSecretRef",
  };
```

### `models.KubernetesExposureCustomBaseDomainCertificateManagedAcmImport`

```typescript
const value:
  models.KubernetesExposureCustomBaseDomainCertificateManagedAcmImport = {
    mode: "managedAcmImport",
  };
```

### `models.KubernetesExposureCustomBaseDomainCertificateAwsAcmArn`

```typescript
const value: models.KubernetesExposureCustomBaseDomainCertificateAwsAcmArn = {
  certificateArn: "<value>",
  mode: "awsAcmArn",
};
```

### `models.KubernetesExposureCustomBaseDomainCertificateManagedTLSSecret`

```typescript
const value:
  models.KubernetesExposureCustomBaseDomainCertificateManagedTLSSecret = {
    mode: "managedTlsSecret",
    secretNameTemplate: "<value>",
  };
```

### `models.KubernetesExposureCustomBaseDomainCertificateNone`

```typescript
const value: models.KubernetesExposureCustomBaseDomainCertificateNone = {
  mode: "none",
};
```

