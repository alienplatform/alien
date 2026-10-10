import { readFile, writeFile } from "node:fs/promises";

// Speakeasy may regroup inline models when optional fields are added. Preserve
// published deep import paths while using the current generated response parsers.
const modules = {
  "createmanagerresponseproviderawsalb2": "createmanagerresponseprovidergkegatewayenum2",
  "createmanagerresponsedomainsunion2": "createmanagerresponsepublicendpointtargetunion2",
  "deploymentdetailresponsependingpreparedstacktypeunion": "deploymentdetailresponsependingpreparedstacktypeenvenum",
  "deploymentdetailresponsepreparedstackoperationsunion": "deploymentdetailresponsepreparedstackoperations",
  "deploymentpendingpreparedstacktypeunion": "deploymentpendingpreparedstacktypeenvenum",
  "deploymentpreparedstackoperationsunion": "deploymentpreparedstackoperations",
  "managerretryresponseendpointaccess2": "managerretryresponsedomains2",
  "managerretryresponseproviderunion2": "managerretryresponseprovidergkegateway2",
  "persistimporteddeploymentrequestpendingpreparedstackcustomsettingsunion": "persistimporteddeploymentrequestpendingpreparedstacksettingscustom1",
  "persistimporteddeploymentrequestpreparedstackoverridestackconditionunion": "persistimporteddeploymentrequestpreparedstackoverrideconditionstack",
  "synclistresponsependingpreparedstacktypeunion": "synclistresponsependingpreparedstacktypeenvenum",
  "synclistresponsepreparedstackoperationsunion": "synclistresponsepreparedstackoperations",
  "releaseinfotypestringlist": "targetdeploymentconfig"
};
// Names can move separately from the shared models in their former module.
const additionalExports = {
  "createmanagerresponseproviderawsalb2": {
    "createmanagerresponse": [
      "CreateManagerResponseProviderAwsAlb2",
      "CreateManagerResponseProviderAwsAlb2$inboundSchema",
      "CreateManagerResponseProviderAwsAlbEnum2",
      "CreateManagerResponseProviderAwsAlbEnum2$inboundSchema",
      "CreateManagerResponseProviderGkeGateway2",
      "CreateManagerResponseProviderGkeGateway2$inboundSchema",
      "createManagerResponseProviderAwsAlb2FromJSON",
      "createManagerResponseProviderGkeGateway2FromJSON"
    ]
  },
  "createmanagerresponsedomainsunion2": {
    "createmanagerresponseprovidergkegatewayenum2": [
      "CreateManagerResponseDomains2",
      "CreateManagerResponseDomains2$inboundSchema",
      "CreateManagerResponseDomainsUnion2",
      "CreateManagerResponseDomainsUnion2$inboundSchema",
      "createManagerResponseDomains2FromJSON",
      "createManagerResponseDomainsUnion2FromJSON"
    ]
  },
  "deploymentdetailresponsependingpreparedstacktypeunion": {
    "deploymentdetailresponsepreparedstackoperations": [
      "DeploymentDetailResponsePendingPreparedStackTypeUnion",
      "DeploymentDetailResponsePendingPreparedStackTypeUnion$inboundSchema",
      "deploymentDetailResponsePendingPreparedStackTypeUnionFromJSON"
    ]
  },
  "deploymentdetailresponsepreparedstackoperationsunion": {
    "deploymentdetailresponse": [
      "DeploymentDetailResponsePreparedStackOperationsUnion",
      "DeploymentDetailResponsePreparedStackOperationsUnion$inboundSchema",
      "deploymentDetailResponsePreparedStackOperationsUnionFromJSON"
    ]
  },
  "deploymentpendingpreparedstacktypeunion": {
    "deploymentpreparedstackoperations": [
      "DeploymentPendingPreparedStackTypeUnion",
      "DeploymentPendingPreparedStackTypeUnion$inboundSchema",
      "deploymentPendingPreparedStackTypeUnionFromJSON"
    ]
  },
  "deploymentpreparedstackoperationsunion": {
    "deployment": [
      "DeploymentPreparedStackOperationsUnion",
      "DeploymentPreparedStackOperationsUnion$inboundSchema",
      "deploymentPreparedStackOperationsUnionFromJSON"
    ]
  },
  "managerretryresponseendpointaccess2": {
    "managerretryresponseprovidergkegateway2": [
      "ManagerRetryResponseDomainsUnion2",
      "ManagerRetryResponseDomainsUnion2$inboundSchema",
      "ManagerRetryResponseEndpointAccess2",
      "ManagerRetryResponseEndpointAccess2$inboundSchema",
      "managerRetryResponseDomainsUnion2FromJSON"
    ]
  },
  "managerretryresponseproviderunion2": {
    "managerretryresponse": [
      "ManagerRetryResponseProviderAwsAlb2",
      "ManagerRetryResponseProviderAwsAlb2$inboundSchema",
      "ManagerRetryResponseProviderAwsAlbEnum2",
      "ManagerRetryResponseProviderAwsAlbEnum2$inboundSchema",
      "ManagerRetryResponseProviderUnion2",
      "ManagerRetryResponseProviderUnion2$inboundSchema",
      "managerRetryResponseProviderAwsAlb2FromJSON",
      "managerRetryResponseProviderUnion2FromJSON"
    ]
  },
  "persistimporteddeploymentrequestpendingpreparedstackcustomsettingsunion": {
    "persistimporteddeploymentrequestpreparedstackoverrideconditionstack": [
      "PersistImportedDeploymentRequestPendingPreparedStackCustomSettingsUnion",
      "PersistImportedDeploymentRequestPendingPreparedStackCustomSettingsUnion$Outbound",
      "PersistImportedDeploymentRequestPendingPreparedStackCustomSettingsUnion$outboundSchema",
      "persistImportedDeploymentRequestPendingPreparedStackCustomSettingsUnionToJSON"
    ]
  },
  "persistimporteddeploymentrequestpreparedstackoverridestackconditionunion": {
    "persistimporteddeploymentrequest": [
      "PersistImportedDeploymentRequestPreparedStackOverrideStackConditionUnion",
      "PersistImportedDeploymentRequestPreparedStackOverrideStackConditionUnion$Outbound",
      "PersistImportedDeploymentRequestPreparedStackOverrideStackConditionUnion$outboundSchema",
      "persistImportedDeploymentRequestPreparedStackOverrideStackConditionUnionToJSON"
    ]
  },
  "synclistresponsependingpreparedstacktypeunion": {
    "synclistresponsepreparedstackoperations": [
      "SyncListResponsePendingPreparedStackTypeUnion",
      "SyncListResponsePendingPreparedStackTypeUnion$inboundSchema",
      "syncListResponsePendingPreparedStackTypeUnionFromJSON"
    ]
  },
  "synclistresponsepreparedstackoperationsunion": {
    "synclistresponse": [
      "SyncListResponsePreparedStackOperationsUnion",
      "SyncListResponsePreparedStackOperationsUnion$inboundSchema",
      "syncListResponsePreparedStackOperationsUnionFromJSON"
    ]
  },
  "releaseinfotypestringlist": {
    "targetdeployment": [
      "ReleaseInfoTypeStringList",
      "ReleaseInfoTypeStringList$inboundSchema"
    ]
  }
};
const models = new URL("../typescript/src/models/", import.meta.url);
for (const [previous, current] of Object.entries(modules)) {
  const existing = await readFile(new URL(`${previous}.ts`, models), "utf8").catch(() => "");
  if (existing && !existing.startsWith("// Generated by generate-compatibility-exports.mjs.")) continue;
  await readFile(new URL(`${current}.ts`, models));
  const reexports = [];
  for (const [module, names] of Object.entries(additionalExports[previous] ?? {})) {
    const source = await readFile(new URL(`${module}.ts`, models), "utf8");
    const values = new Set(
      [...source.matchAll(/export\s+(?:const|function|class)\s+([\w$]+)/g)].map(match => match[1]),
    );
    for (const name of names) {
      const value = values.has(name);
      reexports.push(`export ${value ? "" : "type "}{ ${name} } from "./${module}.js";`);
    }
  }
  await writeFile(
    new URL(`${previous}.ts`, models),
    `// Generated by generate-compatibility-exports.mjs. DO NOT EDIT.\nexport * from "./${current}.js";\n${reexports.join("\n")}\n`,
  );
}
