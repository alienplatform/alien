# TargetDeploymentNetworkUnion


## Supported Types

### `models.TargetDeploymentNetworkUseDefault`

```typescript
const value: models.TargetDeploymentNetworkUseDefault = {
  type: "use-default",
};
```

### `models.TargetDeploymentNetworkCreate`

```typescript
const value: models.TargetDeploymentNetworkCreate = {
  type: "create",
};
```

### `models.TargetDeploymentNetworkByoVpcAws`

```typescript
const value: models.TargetDeploymentNetworkByoVpcAws = {
  privateSubnetIds: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
  publicSubnetIds: [
    "<value 1>",
  ],
  type: "byo-vpc-aws",
  vpcId: "<id>",
};
```

### `models.TargetDeploymentNetworkByoVpcGcp`

```typescript
const value: models.TargetDeploymentNetworkByoVpcGcp = {
  networkName: "<value>",
  region: "<value>",
  subnetName: "<value>",
  type: "byo-vpc-gcp",
};
```

### `models.TargetDeploymentNetworkByoVnetAzure`

```typescript
const value: models.TargetDeploymentNetworkByoVnetAzure = {
  privateSubnetName: "<value>",
  publicSubnetName: "<value>",
  type: "byo-vnet-azure",
  vnetResourceId: "<id>",
};
```

### `any`

```typescript
const value: any = "<value>";
```

