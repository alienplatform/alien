# NewDeploymentRequestNetworkUnion


## Supported Types

### `models.NewDeploymentRequestNetworkUseDefault`

```typescript
const value: models.NewDeploymentRequestNetworkUseDefault = {
  type: "use-default",
};
```

### `models.NewDeploymentRequestNetworkCreate`

```typescript
const value: models.NewDeploymentRequestNetworkCreate = {
  type: "create",
};
```

### `models.NewDeploymentRequestNetworkByoVpcAws`

```typescript
const value: models.NewDeploymentRequestNetworkByoVpcAws = {
  privateSubnetIds: [
    "<value 1>",
  ],
  publicSubnetIds: [
    "<value 1>",
    "<value 2>",
  ],
  type: "byo-vpc-aws",
  vpcId: "<id>",
};
```

### `models.NewDeploymentRequestNetworkByoVpcGcp`

```typescript
const value: models.NewDeploymentRequestNetworkByoVpcGcp = {
  networkName: "<value>",
  region: "<value>",
  subnetName: "<value>",
  type: "byo-vpc-gcp",
};
```

### `models.NewDeploymentRequestNetworkByoVnetAzure`

```typescript
const value: models.NewDeploymentRequestNetworkByoVnetAzure = {
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

