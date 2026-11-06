import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import conventionalCommits from 'conventional-changelog-conventionalcommits';
import { generateNotes as defaultGenerateNotes } from '@semantic-release/release-notes-generator';

const pullRequestSuffix = /\s+\(#(\d+)\)\s*$/;

function pullRequestNumber(commit) {
  return (commit.header || commit.message || '').match(pullRequestSuffix)?.[1] || null;
}

function withoutPullRequest(value, number) {
  return typeof value === 'string' ? value.replace(new RegExp(`\\s+\\(#${number}\\)\\s*$`), '') : value;
}

function authorKey(author) {
  if (!author || typeof author !== 'object') return null;
  const value = author.email || author.name;
  return typeof value === 'string' && value.trim() ? value.trim().toLowerCase() : null;
}

function githubLogin(author) {
  const email = author?.email?.trim();
  return email?.match(/^(?:\d+\+)?([^@]+)@users\.noreply\.github\.com$/i)?.[1] || null;
}

function authorLabel(author, context) {
  const login = githubLogin(author);
  if (login) return `[@${login}](${context.host}/${login})`;
  const name = author?.name?.trim();
  return name?.replace(/[\r\n]+/g, ' ') || null;
}

// Only these sections are published; routine chores stay out of the release notes.
const sectionTitles = [
  { type: 'feat', section: 'Những gì mới' },
  { type: 'fix', section: 'Những gì đã sửa' },
  { type: 'perf', section: 'Hiệu năng' },
  { type: 'revert', section: 'Hoàn tác', hidden: true },
  { type: 'docs', section: 'Tài liệu', hidden: true },
  { type: 'style', section: 'Định dạng', hidden: true },
  { type: 'refactor', section: 'Tái cấu trúc', hidden: true },
  { type: 'test', section: 'Kiểm thử', hidden: true },
  { type: 'build', section: 'Build', hidden: true },
  { type: 'ci', section: 'CI', hidden: true },
  { type: 'chore', section: 'Việc khác', hidden: true },
];

function readPinnedVersions() {
  const read = file => {
    try {
      return readFileSync(file, 'utf8');
    } catch {
      return '';
    }
  };
  const cargo = read('Cargo.toml');
  const client = cargo.match(/^version = "([^"]+)"/m)?.[1] ?? 'chưa xác định';
  const rust = read('rust-toolchain.toml').match(/channel = "([^"]+)"/)?.[1] ?? 'chưa xác định';
  const lock = read('Cargo.lock');
  const locked = name => lock.match(new RegExp(`name = "${name}"\\nversion = "([^"]+)"`))?.[1] ?? 'chưa xác định';
  return {
    client,
    rust,
    fluentBundle: locked('fluent-bundle'),
    fluentTemplates: locked('fluent-templates'),
  };
}

function versionsSection() {
  const v = readPinnedVersions();
  return [
    '### Phiên bản',
    '',
    '| Thành phần | Phiên bản |',
    '| --- | --- |',
    `| Bytwina | ${v.client} |`,
    `| Rust | ${v.rust} |`,
    `| fluent-bundle | ${v.fluentBundle} |`,
    `| fluent-templates | ${v.fluentTemplates} |`,
  ].join('\n');
}

function introNotes(context) {
  const v = readPinnedVersions();
  const version = context?.nextRelease?.version ?? v.client;
  return [
    '## Bytwina',
    '',
    'Bytwina là client Discord chạy native, viết bằng Rust với egui và wgpu. Không Electron,',
    'không webview cho phần giao diện, nên bộ nhớ và CPU thấp hơn hẳn bản web, trong khi vẫn',
    'giữ đủ chat, voice và extension.',
    '',
    '### Tính năng',
    '',
    '- Giao diện native, render bằng GPU; cuộn mượt trên máy yếu.',
    '- Voice và video call: Opus, DAVE/E2EE (không có đường lui không mã hoá).',
    '- Server, DM, thread, forum, reaction, poll, attachment, định dạng Markdown đầy đủ.',
    '- Extension và theme cài thêm được, chạy WASM trong sandbox.',
    '- 13 ngôn ngữ, giao diện tiếng Việt có sẵn.',
    '- Token lưu trong credential store của hệ điều hành; không telemetry, không crash uploader.',
    '- Tự cập nhật có kiểm tra SHA-256 trước khi cài.',
    '',
    '### Cài đặt',
    '',
    '1. Tải `Bytwina-<version>-setup.exe` trong mục Assets bên dưới.',
    '2. Chạy file, chọn thư mục cài đặt.',
    '3. Mở Bytwina và đăng nhập bằng tài khoản Discord đang dùng.',
    '',
    'Bản portable: tải file `.zip`, giải nén và chạy `bytwina.exe`.',
    '',
    '### Yêu cầu',
    '',
    '- Windows 10 hoặc Windows 11, 64-bit.',
    '- Không cần cài .NET hay WebView2 riêng.',
    '',
    '### Lưu ý',
    '',
    'Bytwina là client không chính thức, không liên kết với Discord. Tự động hoá tài khoản',
    'người dùng là hành vi vi phạm Terms of Service của Discord; bạn dùng với rủi ro của mình.',
    '',
    `_Bản phát hành ${version}._`,
    '',
    versionsSection(),
  ].join('\n');
}

// A first release is announced with the introduction, every later one with the diff.
// gh is present on runners; offline checks override the choice explicitly.
function firstRelease(context) {
  const forced = process.env.BYTWINA_NOTES_MODE;
  if (forced === 'intro') return true;
  if (forced === 'conventional') return false;
  try {
    const listed = execFileSync('gh', ['release', 'list', '--limit', '1'], {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'ignore'],
    });
    return listed.trim() === '';
  } catch {
    return getLatestReleaseTag(context?.nextRelease?.gitHead || 'HEAD') === null;
  }
}

function previousAuthors(ref) {
  if (!ref) return new Set();
  try {
    return new Set(
      execFileSync('git', ['log', ref, '--format=%an%x09%ae'], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] })
        .split('\n')
        .map(line => {
          const [name, email] = line.split('\t');
          return authorKey({ name, email });
        })
        .filter(Boolean),
    );
  } catch {
    return new Set();
  }
}

async function generateAnnotatedNotes(pluginConfig, context, previousRef) {
  const presetConfig = { ...pluginConfig.presetConfig, types: sectionTitles };
  const preset = await conventionalCommits(presetConfig);
  const baseTransform = preset.writer.transform;
  const previous = previousAuthors(previousRef);
  const writerOpts = {
    ...pluginConfig.writerOpts,
    transform(commit, writerContext) {
      const pullRequest = pullRequestNumber(commit);
      const parsedCommit = pullRequest
        ? {
            ...commit,
            header: withoutPullRequest(commit.header, pullRequest),
            subject: withoutPullRequest(commit.subject, pullRequest),
            references: commit.references.filter(reference => String(reference.issue) !== pullRequest),
          }
        : commit;
      const transformed = baseTransform(parsedCommit, writerContext);
      if (!transformed) return transformed;
      return {
        ...transformed,
        author: authorLabel(commit.author, writerContext),
        authorKey: authorKey(commit.author),
        pullRequest,
        ...(pullRequest && { header: parsedCommit.header }),
      };
    },
    commitPartial: preset.writer.commitPartial.replace(
      '{{~!-- commit link --}}',
      '{{~#if author}} by {{author}}{{~/if}}{{~#if pullRequest}} in [#{{pullRequest}}]({{~@root.host}}/{{~@root.owner}}/{{~@root.repository}}/pull/{{pullRequest}}){{~/if}}\n\n{{~!-- commit link --}}',
    ),
    mainTemplate: `{{> header}}
{{#if noteGroups}}
{{#each noteGroups}}

### ⚠ {{title}}

{{#each notes}}
* {{#if commit.scope}}**{{commit.scope}}:** {{/if}}{{text}}
{{/each}}
{{/each}}
{{/if}}
{{#each commitGroups}}

{{#if title}}
### {{title}}

{{/if}}
{{#each commits}}
{{> commit root=@root}}
{{/each}}
{{/each}}
{{#if newContributors}}

## New Contributors

{{#each newContributors}}
* {{text}}
{{/each}}
{{/if}}
`,
    finalizeContext(templateContext, _options, commits) {
      const newContributors = [];
      const seen = new Set();
      for (const commit of commits) {
        if (!commit.pullRequest || !commit.authorKey || previous.has(commit.authorKey) || seen.has(commit.authorKey)) continue;
        if (commit.author?.includes('[bot]')) continue;
        seen.add(commit.authorKey);
        const url = `${templateContext.host}/${templateContext.owner}/${templateContext.repository}/pull/${commit.pullRequest}`;
        newContributors.push({ text: `${commit.author} made their first contribution in [#${commit.pullRequest}](${url})` });
      }
      return { ...templateContext, newContributors };
    },
  };
  const notes = await defaultGenerateNotes({ ...pluginConfig, presetConfig, writerOpts }, context);
  return [notes.trimEnd(), '', versionsSection()].join('\n');
}

export function getLatestReleaseTag(ref = 'HEAD') {
  try {
    execFileSync('git', ['describe', '--tags', '--match', 'v[0-9]*', '--exact-match', ref], { stdio: 'ignore' });
    return execFileSync('git', ['describe', '--tags', '--match', 'v[0-9]*', '--abbrev=0', `${ref}~1`], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }).trim();
  } catch {
    try {
      return execFileSync('git', ['describe', '--tags', '--match', 'v[0-9]*', '--abbrev=0', ref], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }).trim();
    } catch {
      return null;
    }
  }
}

export async function generateReleaseNotes(channel, pluginConfig, context) {
  if (firstRelease(context)) return introNotes(context);

  if (channel === 'production') {
    return generateAnnotatedNotes(pluginConfig, context, context.lastRelease?.gitTag || context.lastRelease?.gitHead);
  }

  const targetRef = context.nextRelease?.gitHead || 'HEAD';
  const latestTag = getLatestReleaseTag(targetRef);

  if (!latestTag) {
    return generateAnnotatedNotes(pluginConfig, context, context.lastRelease?.gitTag || context.lastRelease?.gitHead);
  }

  let nightlyHashes;
  try {
    nightlyHashes = new Set(
      execFileSync('git', ['rev-list', `${latestTag}..${targetRef}`], { encoding: 'utf8' })
        .split('\n')
        .map(h => h.trim())
        .filter(Boolean)
    );
  } catch {
    return generateAnnotatedNotes(pluginConfig, context, latestTag);
  }

  const nightlyCommits = (context.commits || []).filter(c => nightlyHashes.has(c.hash));
  const date = new Date().toISOString().slice(0, 10).replace(/-/g, '');
  const runNumber = process.env.GITHUB_RUN_NUMBER;
  const nightlyVersion = context.nextRelease?.version?.includes('nightly')
    ? context.nextRelease.version
    : (runNumber ? `${context.nextRelease.version}-nightly.${date}.${runNumber}` : `${context.nextRelease.version}-nightly`);

  return generateAnnotatedNotes(pluginConfig, {
    ...context,
    commits: nightlyCommits,
    lastRelease: {
      gitTag: latestTag,
      gitHead: latestTag,
      version: latestTag.replace(/^v/, ''),
    },
    nextRelease: {
      ...context.nextRelease,
      version: nightlyVersion,
      gitTag: `v${nightlyVersion}`,
    },
  }, latestTag);
}
