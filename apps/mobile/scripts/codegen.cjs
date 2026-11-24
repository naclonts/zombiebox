const path = require('node:path');
const {combineSchemasInFileList} = require('@react-native/codegen/lib/cli/combine/combine-js-to-schema');
const {generate} = require('@react-native/codegen/lib/generators/RNCodegen');
const {codegenConfig} = require('../package.json');

const root = path.join(__dirname, '..');
const specs = path.join(root, codegenConfig.jsSrcsDir);
for (const [platform, generator] of [
  ['android', 'componentsAndroid'],
  ['ios', 'componentsIOS'],
]) {
  const schema = combineSchemasInFileList([specs], platform, null, codegenConfig.name);
  if (!schema.modules.ZombieboxSurface) {
    throw new Error(`Missing ZombieboxSurface in the ${platform} schema`);
  }
  const outputDirectory = path.join(root, 'build', platform);
  const success = generate({
    libraryName: codegenConfig.name,
    packageName: codegenConfig.android.javaPackageName,
    schema,
    outputDirectory,
  }, {generators: [generator]});
  if (!success) {
    throw new Error(`Failed to write ${platform} bindings`);
  }
  console.log(`Generated ${platform} bindings in ${outputDirectory}`);
}
