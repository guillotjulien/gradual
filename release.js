// release.js
const fs = require('fs');
const { execSync } = require('child_process');

const newVersion = process.argv[2];

if (!newVersion || !/^\d+\.\d+\.\d+/.test(newVersion)) {
  console.error("Usage: node release.js <version> (e.g., 0.1.1)");
  process.exit(1);
}

console.log(`Bumping to v${newVersion}...`);

// 1. Update Cargo.toml
let cargoToml = fs.readFileSync('Cargo.toml', 'utf8');
cargoToml = cargoToml.replace(/^version\s*=\s*".*?"/m, `version = "${newVersion}"`);
fs.writeFileSync('Cargo.toml', cargoToml);

// Update Cargo.lock to match the new Cargo.toml version
execSync('cargo update -p gradual', { stdio: 'inherit' }); 

// 2. Update package.json and its optional dependencies
const pkgPath = 'npm/app/package.json';
const pkg = JSON.parse(fs.readFileSync(pkgPath, 'utf8'));

pkg.version = newVersion;

if (pkg.optionalDependencies) {
  for (const dep in pkg.optionalDependencies) {
    if (dep.startsWith('@julienguillot/gradual-')) {
      pkg.optionalDependencies[dep] = newVersion;
    }
  }
}

fs.writeFileSync(pkgPath, JSON.stringify(pkg, null, 2) + '\n');

// 3. Commit and Tag
execSync('git add Cargo.toml Cargo.lock npm/app/package.json', { stdio: 'inherit' });
execSync(`git commit -m "chore: release v${newVersion}"`, { stdio: 'inherit' });
execSync(`git tag ${newVersion}`, { stdio: 'inherit' });

console.log(`\n✅ Successfully bumped to v${newVersion} and tagged.`);
console.log(`🚀 Run 'git push && git push --tags' to trigger the CI.`);