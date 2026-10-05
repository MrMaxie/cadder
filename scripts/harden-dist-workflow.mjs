import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readFile, writeFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const projectDirectory = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const workflowPath = resolve(projectDirectory, '.github', 'workflows', 'release.yml');

function lines(...values) {
  return values.join('\n');
}

function replaceOnce(source, expected, replacement, label) {
  const first = source.indexOf(expected);
  assert.notEqual(first, -1, `cargo-dist output is missing the expected ${label} fragment`);
  assert.equal(
    source.indexOf(expected, first + expected.length),
    -1,
    `cargo-dist output contains more than one ${label} fragment`,
  );
  return `${source.slice(0, first)}${replacement}${source.slice(first + expected.length)}`;
}

export function hardenGeneratedWorkflow(generated) {
  let source = replaceOnce(
    generated,
    lines('permissions:', '  "contents": "write"'),
    lines('permissions:', '  "contents": "read"'),
    'root permission',
  );
  source = replaceOnce(
    source,
    lines(
      '    outputs:',
      '      val: ${{ steps.plan.outputs.manifest }}',
      "      tag: ${{ !github.event.pull_request && github.ref_name || '' }}",
      "      tag-flag: ${{ !github.event.pull_request && format('--tag={0}', github.ref_name) || '' }}",
      '      publishing: ${{ !github.event.pull_request }}',
      '    env:',
      '      GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}',
    ),
    lines(
      '    outputs:',
      '      val: ${{ steps.plan.outputs.manifest }}',
      '      tag: ${{ steps.plan.outputs.tag }}',
      '      publishing: ${{ !github.event.pull_request }}',
      '    env:',
      "      RELEASE_TAG: ${{ !github.event.pull_request && github.ref_name || '' }}",
    ),
    'plan outputs and environment',
  );
  source = replaceOnce(
    source,
    lines(
      '      - id: plan',
      '        run: |',
      "          dist ${{ (!github.event.pull_request && format('host --steps=create --tag={0}', github.ref_name)) || 'plan' }} --output-format=json > plan-dist-manifest.json",
      '          echo "dist ran successfully"',
      '          cat plan-dist-manifest.json',
      '          echo "manifest=$(jq -c "." plan-dist-manifest.json)" >> "$GITHUB_OUTPUT"',
    ),
    lines(
      '      - id: plan',
      '        shell: bash',
      '        run: |',
      '          if [[ -n "$RELEASE_TAG" ]]; then',
      '            if [[ ! "$RELEASE_TAG" =~ ^v(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)(-[0-9A-Za-z.-]+)?(\\+[0-9A-Za-z.-]+)?$ ]]; then',
      '              echo "Invalid Cadder release tag: $RELEASE_TAG" >&2',
      '              exit 1',
      '            fi',
      '            dist host --allow-dirty --steps=create --tag "$RELEASE_TAG" --output-format=json > plan-dist-manifest.json',
      '          else',
      '            dist plan --allow-dirty --output-format=json > plan-dist-manifest.json',
      '          fi',
      '          echo "dist ran successfully"',
      '          cat plan-dist-manifest.json',
      '          echo "tag=$RELEASE_TAG" >> "$GITHUB_OUTPUT"',
      '          echo "manifest=$(jq -c "." plan-dist-manifest.json)" >> "$GITHUB_OUTPUT"',
    ),
    'plan command',
  );
  source = replaceOnce(
    source,
    lines(
      '    env:',
      '      GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}',
      "      BUILD_MANIFEST_NAME: target/distrib/${{ join(matrix.targets, '-') }}-dist-manifest.json",
    ),
    lines(
      '    env:',
      '      RELEASE_TAG: ${{ needs.plan.outputs.tag }}',
      "      BUILD_MANIFEST_NAME: target/distrib/${{ join(matrix.targets, '-') }}-dist-manifest.json",
    ),
    'local build environment',
  );
  source = replaceOnce(
    source,
    lines(
      '      - name: Build artifacts',
      '        run: |',
      '          # Actually do builds and make zips and whatnot',
      '          dist build ${{ needs.plan.outputs.tag-flag }} --print=linkage --output-format=json ${{ matrix.dist_args }} > dist-manifest.json',
      '          echo "dist ran successfully"',
    ),
    lines(
      '      - name: Build artifacts',
      '        shell: bash',
      '        run: |',
      '          # Actually do builds and make zips and whatnot',
      '          tag_args=()',
      '          if [[ -n "$RELEASE_TAG" ]]; then tag_args=(--tag "$RELEASE_TAG"); fi',
      '          dist build --allow-dirty "${tag_args[@]}" --print=linkage --output-format=json ${{ matrix.dist_args }} > dist-manifest.json',
      '          echo "dist ran successfully"',
    ),
    'local build command',
  );
  source = replaceOnce(
    source,
    lines(
      '    env:',
      '      GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}',
      '      BUILD_MANIFEST_NAME: target/distrib/global-dist-manifest.json',
    ),
    lines(
      '    env:',
      '      RELEASE_TAG: ${{ needs.plan.outputs.tag }}',
      '      BUILD_MANIFEST_NAME: target/distrib/global-dist-manifest.json',
    ),
    'global build environment',
  );
  source = replaceOnce(
    source,
    '          dist build ${{ needs.plan.outputs.tag-flag }} --output-format=json "--artifacts=global" > dist-manifest.json',
    lines(
      '          tag_args=()',
      '          if [[ -n "$RELEASE_TAG" ]]; then tag_args=(--tag "$RELEASE_TAG"); fi',
      '          dist build --allow-dirty "${tag_args[@]}" --output-format=json "--artifacts=global" > dist-manifest.json',
    ),
    'global build command',
  );
  source = replaceOnce(
    source,
    lines('    env:', '      GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}', '    runs-on: "ubuntu-22.04"'),
    lines('    env:', '      RELEASE_TAG: ${{ needs.plan.outputs.tag }}', '    runs-on: "ubuntu-22.04"'),
    'host environment',
  );
  source = replaceOnce(
    source,
    '          dist host ${{ needs.plan.outputs.tag-flag }} --steps=upload --steps=release --output-format=json > dist-manifest.json',
    '          dist host --allow-dirty --tag "$RELEASE_TAG" --steps=upload --steps=release --output-format=json > dist-manifest.json',
    'host command',
  );
  source = replaceOnce(
    source,
    lines(
      '  custom-verify-release-artifacts:',
      '    needs:',
      '      - plan',
      '      - host',
      "    if: ${{ !fromJson(needs.plan.outputs.val).announcement_is_prerelease || fromJson(needs.plan.outputs.val).publish_prereleases }}",
    ),
    lines(
      '  custom-verify-release-artifacts:',
      '    needs:',
      '      - plan',
      '      - build-global-artifacts',
      "    if: ${{ always() && needs.plan.result == 'success' && needs.build-global-artifacts.result == 'success' && (!fromJson(needs.plan.outputs.val).announcement_is_prerelease || fromJson(needs.plan.outputs.val).publish_prereleases) }}",
    ),
    'release artifact verification dependencies',
  );
  source = replaceOnce(
    source,
    '          RELEASE_COMMIT: "${{ github.sha }}"',
    lines(
      '          RELEASE_COMMIT: "${{ github.sha }}"',
      '          RELEASE_TAG: "${{ needs.plan.outputs.tag }}"',
    ),
    'announce environment',
  );
  source = replaceOnce(
    source,
    '          gh release create "${{ needs.plan.outputs.tag }}" --target "$RELEASE_COMMIT" $PRERELEASE_FLAG --title "$ANNOUNCEMENT_TITLE" --notes-file "$RUNNER_TEMP/notes.txt" artifacts/*',
    '          gh release create "$RELEASE_TAG" --target "$RELEASE_COMMIT" $PRERELEASE_FLAG --title "$ANNOUNCEMENT_TITLE" --notes-file "$RUNNER_TEMP/notes.txt" artifacts/*',
    'announce command',
  );
  validateHardenedWorkflow(source);
  return source;
}

export function validateHardenedWorkflow(source) {
  assert.match(source, /permissions:\n  "contents": "read"/);
  assert.equal(source.match(/GH_TOKEN: \$\{\{ secrets\.GITHUB_TOKEN \}\}/g)?.length, 1);
  assert.equal(source.match(/"contents": "write"/g)?.length, 1);
  assert.equal(source.match(/github\.ref_name/g)?.length, 1);
  assert.doesNotMatch(source, /tag-flag/);
  assert.doesNotMatch(source, /run:[^]*?github\.ref_name/);
  assert.equal(source.match(/dist (?:plan|build|host) --allow-dirty/g)?.length, 5);
  assert.match(source, /RELEASE_TAG: \$\{\{ needs\.plan\.outputs\.tag \}\}/);
  assert.match(source, /gh release create "\$RELEASE_TAG"/);
  assert.match(source, /if \[\[ ! "\$RELEASE_TAG" =~ \^v/);
  assert.match(
    source,
    /custom-verify-release-artifacts:\n    needs:\n      - plan\n      - build-global-artifacts\n    if: \$\{\{ always\(\) && needs\.plan\.result == 'success' && needs\.build-global-artifacts\.result == 'success'/,
  );
}

async function apply() {
  const generated = await readFile(workflowPath, 'utf8');
  await writeFile(workflowPath, hardenGeneratedWorkflow(generated), 'utf8');
}

async function checkGenerated() {
  const committed = await readFile(workflowPath);
  try {
    const result = spawnSync('dist', ['generate', '--mode', 'ci'], {
      cwd: projectDirectory,
      encoding: 'utf8',
      shell: false,
    });
    if (result.error) throw result.error;
    if (result.status !== 0) {
      throw new Error(`dist generate failed with status ${result.status}: ${result.stderr}`);
    }
    const generated = await readFile(workflowPath, 'utf8');
    const expected = Buffer.from(hardenGeneratedWorkflow(generated), 'utf8');
    assert.deepEqual(
      committed,
      expected,
      'release.yml differs from cargo-dist output after the required security hardening pass',
    );
  } finally {
    await writeFile(workflowPath, committed);
  }
}

if (resolve(process.argv[1] ?? '') === fileURLToPath(import.meta.url)) {
  const mode = process.argv[2];
  const operation = mode === '--apply' ? apply : mode === '--check-generated' ? checkGenerated : null;
  if (!operation) {
    process.stderr.write('usage: node scripts/harden-dist-workflow.mjs --apply|--check-generated\n');
    process.exitCode = 2;
  } else {
    operation().catch((error) => {
      process.stderr.write(`${error.stack ?? error}\n`);
      process.exitCode = 1;
    });
  }
}
