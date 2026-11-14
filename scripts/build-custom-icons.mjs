import fs from 'node:fs';
import path from 'node:path';

const outDir = 'frontend/public/icons/installapps';

function template({
  prefix,
  bgColors: [bg0, bg52, bg100],
  glowTop: [glowTopCol, glowTopMid = bg52],
  glowBottom: [glowBotCol],
  rim: [rimTop, rimMid, rimDark, rimBot],
  content
}) {
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256" width="256" height="256" fill="none">
  <defs>
    <filter id="${prefix}-shadow" x="20" y="20" width="216" height="220" filterUnits="userSpaceOnUse">
      <feDropShadow dx="0" dy="6" stdDeviation="6" flood-color="#020617" flood-opacity="0.72" />
    </filter>
    <linearGradient id="${prefix}-bg" x1="0" y1="0" x2="256" y2="256" gradientUnits="userSpaceOnUse">
      <stop offset="0%" stop-color="${bg0}" />
      <stop offset="52%" stop-color="${bg52}" />
      <stop offset="100%" stop-color="${bg100}" />
    </linearGradient>
    <radialGradient id="${prefix}-glow-top" cx="28%" cy="18%" r="78%">
      <stop offset="0%" stop-color="${glowTopCol}" stop-opacity="0.42" />
      <stop offset="55%" stop-color="${glowTopMid}" stop-opacity="0.16" />
      <stop offset="100%" stop-color="${bg52}" stop-opacity="0" />
    </radialGradient>
    <radialGradient id="${prefix}-glow-bottom" cx="82%" cy="86%" r="62%">
      <stop offset="0%" stop-color="${glowBotCol}" stop-opacity="0.25" />
      <stop offset="100%" stop-color="${bg52}" stop-opacity="0" />
    </radialGradient>
    <linearGradient id="${prefix}-rim" x1="128" y1="0" x2="128" y2="256" gradientUnits="userSpaceOnUse">
      <stop offset="0%" stop-color="${rimTop}" stop-opacity="0.65" />
      <stop offset="25%" stop-color="${rimMid}" stop-opacity="0.32" />
      <stop offset="80%" stop-color="${rimDark}" stop-opacity="0.24" />
      <stop offset="100%" stop-color="${rimBot}" stop-opacity="0.42" />
    </linearGradient>
    <linearGradient id="${prefix}-sheen" x1="128" y1="0" x2="128" y2="115" gradientUnits="userSpaceOnUse">
      <stop offset="0%" stop-color="#FFFFFF" stop-opacity="0.16" />
      <stop offset="100%" stop-color="#FFFFFF" stop-opacity="0" />
    </linearGradient>
    <clipPath id="${prefix}-clip">
      <rect x="0" y="0" width="256" height="256" rx="58" />
    </clipPath>
  </defs>
  <g clip-path="url(#${prefix}-clip)">
    <rect x="0" y="0" width="256" height="256" fill="url(#${prefix}-bg)" />
    <rect x="0" y="0" width="256" height="256" fill="url(#${prefix}-glow-top)" />
    <rect x="0" y="0" width="256" height="256" fill="url(#${prefix}-glow-bottom)" />
    <ellipse cx="128" cy="4" rx="126" ry="64" fill="url(#${prefix}-sheen)" />
  </g>
  <rect x="1.5" y="1.5" width="253" height="253" rx="56.5" stroke="url(#${prefix}-rim)" stroke-width="3" />
  <g filter="url(#${prefix}-shadow)">
    ${content}
  </g>
</svg>
`;
}

async function main() {
  console.log('Generating custom icons...');

  // 1. GitHub Copilot
  const copilotSvg = await fetch('https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/githubcopilot.svg').then(r => r.text());
  const copilotPathMatch = copilotSvg.match(/<path\s+d="([^"]+)"/);
  const copilotPath = copilotPathMatch ? copilotPathMatch[1] : '';

  fs.writeFileSync(path.join(outDir, 'GitHub-Copilot.svg'), template({
    prefix: 'cpt',
    bgColors: ['#20103A', '#130825', '#080310'],
    glowTop: ['#A855F7', '#6D28D9'],
    glowBottom: ['#EC4899'],
    rim: ['#F3E8FF', '#A855F7', '#3B0764', '#EC4899'],
    content: `<svg x="50" y="50" width="156" height="156" viewBox="0 0 24 24" preserveAspectRatio="xMidYMid meet">
      <defs>
        <linearGradient id="cpt-mark" x1="0" y1="0" x2="24" y2="24" gradientUnits="userSpaceOnUse">
          <stop offset="0%" stop-color="#FFFFFF" />
          <stop offset="100%" stop-color="#C084FC" />
        </linearGradient>
      </defs>
      <path fill="url(#cpt-mark)" d="${copilotPath}" />
    </svg>`
  }));

  // 2. Visual Studio
  const vsSvg = await fetch('https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/visualstudio.svg').then(r => r.text());
  const vsPathMatch = vsSvg.match(/<path\s+d="([^"]+)"/);
  const vsPath = vsPathMatch ? vsPathMatch[1] : '';

  fs.writeFileSync(path.join(outDir, 'Visual-Studio.svg'), template({
    prefix: 'vs',
    bgColors: ['#281042', '#160826', '#090311'],
    glowTop: ['#9333EA', '#7C3AED'],
    glowBottom: ['#C084FC'],
    rim: ['#F3E8FF', '#9333EA', '#3B0764', '#C084FC'],
    content: `<svg x="50" y="50" width="156" height="156" viewBox="0 0 24 24" preserveAspectRatio="xMidYMid meet">
      <defs>
        <linearGradient id="vs-mark" x1="0" y1="0" x2="24" y2="24" gradientUnits="userSpaceOnUse">
          <stop offset="0%" stop-color="#D8B4FE" />
          <stop offset="50%" stop-color="#A855F7" />
          <stop offset="100%" stop-color="#7E22CE" />
        </linearGradient>
      </defs>
      <path fill="url(#vs-mark)" d="${vsPath}" />
    </svg>`
  }));

  // 3. Microsoft .NET
  const dnetSvg = await fetch('https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/dotnet.svg').then(r => r.text());
  const dnetPathMatch = dnetSvg.match(/<path\s+d="([^"]+)"/);
  const dnetPath = dnetPathMatch ? dnetPathMatch[1] : '';

  fs.writeFileSync(path.join(outDir, 'Microsoft-DotNet.svg'), template({
    prefix: 'dnet',
    bgColors: ['#1C0E40', '#100726', '#070213'],
    glowTop: ['#512BD4', '#4338CA'],
    glowBottom: ['#8B5CF6'],
    rim: ['#DDD6FE', '#512BD4', '#2E1065', '#8B5CF6'],
    content: `<svg x="46" y="46" width="164" height="164" viewBox="0 0 128 128" preserveAspectRatio="xMidYMid meet">
      <defs>
        <linearGradient id="dnet-circle" x1="0" y1="0" x2="128" y2="128" gradientUnits="userSpaceOnUse">
          <stop offset="0%" stop-color="#6739EE" />
          <stop offset="100%" stop-color="#512BD4" />
        </linearGradient>
      </defs>
      <circle cx="64" cy="64" r="60" fill="url(#dnet-circle)" />
      <g transform="translate(18, 18) scale(3.833)">
        <path fill="#FFFFFF" d="${dnetPath}" />
      </g>
    </svg>`
  }));

  // 4. Visual C++ Redistributable
  const cppSvg = await fetch('https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/cplusplus.svg').then(r => r.text());
  const cppPathMatch = cppSvg.match(/<path\s+d="([^"]+)"/);
  const cppPath = cppPathMatch ? cppPathMatch[1] : '';

  fs.writeFileSync(path.join(outDir, 'Visual-Cpp-Redist.svg'), template({
    prefix: 'vc',
    bgColors: ['#0A1F3D', '#061224', '#020712'],
    glowTop: ['#00599C', '#0284C7'],
    glowBottom: ['#38BDF8'],
    rim: ['#BAE6FD', '#00599C', '#082F49', '#38BDF8'],
    content: `<svg x="50" y="50" width="156" height="156" viewBox="0 0 24 24" preserveAspectRatio="xMidYMid meet">
      <defs>
        <linearGradient id="vc-mark" x1="0" y1="0" x2="24" y2="24" gradientUnits="userSpaceOnUse">
          <stop offset="0%" stop-color="#38BDF8" />
          <stop offset="100%" stop-color="#00599C" />
        </linearGradient>
      </defs>
      <path fill="url(#vc-mark)" d="${cppPath}" />
    </svg>`
  }));

  // 5. Roblox
  const robloxSvg = await fetch('https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/roblox.svg').then(r => r.text());
  const robloxPathMatch = robloxSvg.match(/<path\s+d="([^"]+)"/);
  const robloxPath = robloxPathMatch ? robloxPathMatch[1] : '';

  fs.writeFileSync(path.join(outDir, 'Roblox.svg'), template({
    prefix: 'rbx',
    bgColors: ['#1C1E24', '#101115', '#08080A'],
    glowTop: ['#94A3B8', '#475569'],
    glowBottom: ['#EF4444'],
    rim: ['#F1F5F9', '#94A3B8', '#1E293B', '#EF4444'],
    content: `<svg x="52" y="52" width="152" height="152" viewBox="0 0 24 24" preserveAspectRatio="xMidYMid meet">
      <defs>
        <linearGradient id="rbx-mark" x1="0" y1="0" x2="24" y2="24" gradientUnits="userSpaceOnUse">
          <stop offset="0%" stop-color="#FFFFFF" />
          <stop offset="100%" stop-color="#CBD5E1" />
        </linearGradient>
      </defs>
      <path fill="url(#rbx-mark)" d="${robloxPath}" />
    </svg>`
  }));

  // 6. OpenSCAD
  const oscSvg = await fetch('https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/openscad.svg').then(r => r.text());
  const oscPathMatch = oscSvg.match(/<path\s+d="([^"]+)"/);
  const oscPath = oscPathMatch ? oscPathMatch[1] : '';

  fs.writeFileSync(path.join(outDir, 'OpenSCAD.svg'), template({
    prefix: 'osc',
    bgColors: ['#291E0A', '#171004', '#0B0802'],
    glowTop: ['#F59E0B', '#B45309'],
    glowBottom: ['#FBBF24'],
    rim: ['#FEF08A', '#F59E0B', '#451A03', '#FBBF24'],
    content: `<svg x="50" y="50" width="156" height="156" viewBox="0 0 24 24" preserveAspectRatio="xMidYMid meet">
      <defs>
        <linearGradient id="osc-mark" x1="0" y1="0" x2="24" y2="24" gradientUnits="userSpaceOnUse">
          <stop offset="0%" stop-color="#FDE047" />
          <stop offset="50%" stop-color="#F59E0B" />
          <stop offset="100%" stop-color="#D97706" />
        </linearGradient>
      </defs>
      <path fill="url(#osc-mark)" d="${oscPath}" />
    </svg>`
  }));

  // 7. Grok / xAI
  const xSvg = await fetch('https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/x.svg').then(r => r.text());
  const xPathMatch = xSvg.match(/<path\s+d="([^"]+)"/);
  const xPath = xPathMatch ? xPathMatch[1] : '';

  fs.writeFileSync(path.join(outDir, 'Grok.svg'), template({
    prefix: 'grk',
    bgColors: ['#161619', '#0B0B0D', '#040405'],
    glowTop: ['#FFFFFF', '#94A3B8'],
    glowBottom: ['#64748B'],
    rim: ['#FFFFFF', '#94A3B8', '#1E293B', '#64748B'],
    content: `<svg x="52" y="52" width="152" height="152" viewBox="0 0 24 24" preserveAspectRatio="xMidYMid meet">
      <path fill="#FFFFFF" d="${xPath}" />
    </svg>`
  }));

  // 8. OpenAL
  // The iconic Creative Technology OpenAL badge:
  // Deep red plate with the distinctive bold rounded "AL" typography (featuring the signature upward flick on L)
  fs.writeFileSync(path.join(outDir, 'OpenAL.svg'), template({
    prefix: 'oal',
    bgColors: ['#2F0D0E', '#190607', '#0B0203'],
    glowTop: ['#DC2626', '#991B1B'],
    glowBottom: ['#EF4444'],
    rim: ['#FECACA', '#DC2626', '#450A0A', '#EF4444'],
    content: `
      <defs>
        <linearGradient id="oal-badge" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stop-color="#B91C1C" />
          <stop offset="100%" stop-color="#7F1D1D" />
        </linearGradient>
        <linearGradient id="oal-text" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stop-color="#FFFFFF" />
          <stop offset="100%" stop-color="#F3F4F6" />
        </linearGradient>
        <filter id="oal-inset" x="-10%" y="-10%" width="120%" height="120%">
          <feDropShadow dx="0" dy="2" stdDeviation="2" flood-color="#000000" flood-opacity="0.3" />
        </filter>
      </defs>
      <!-- Inner Crimson Badge Plate -->
      <rect x="48" y="48" width="160" height="160" rx="36" fill="url(#oal-badge)" stroke="#EF4444" stroke-width="2.5" />
      <!-- Authentic OpenAL 'A' and 'L' Typography -->
      <g filter="url(#oal-inset)">
        <!-- Letter 'A' -->
        <path fill="url(#oal-text)" fill-rule="evenodd" d="
          M 103 84
          C 94 84, 82 89, 75 97
          C 68 105, 64 116, 64 128
          L 64 172
          C 64 174.5, 66.5 177, 69.5 177
          L 83 177
          C 86 177, 88.5 174.5, 88.5 172
          L 88.5 152
          L 117.5 152
          L 117.5 172
          C 117.5 174.5, 120 177, 123 177
          L 136.5 177
          C 139.5 177, 142 174.5, 142 172
          L 142 128
          C 142 116, 138 105, 131 97
          C 124 89, 112 84, 103 84 Z
          M 88.5 134
          L 88.5 125
          C 88.5 119, 91 113, 95 109
          C 99 105, 104 103, 108 103
          C 112 103, 117 105, 121 109
          C 125 113, 127.5 119, 127.5 125
          L 127.5 134 Z
        "/>
        <!-- Letter 'L' with the signature OpenAL upward curved flick -->
        <path fill="url(#oal-text)" d="
          M 152 89
          C 152 86.5, 154.5 84, 157.5 84
          L 171 84
          C 174 84, 176.5 86.5, 176.5 89
          L 176.5 152
          C 176.5 156, 180 159, 185 159
          C 190 159, 195 156, 198 150
          C 200 146, 203 143, 206 143
          C 209 143, 211 146, 211 150
          C 210 162, 201 172, 190 176
          C 183 178, 173 178, 166 174
          C 157 169, 152 161, 152 152 Z
        "/>
      </g>
    `
  }));

  // 9. GitHub CLI
  const ghSvg = await fetch('https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/github.svg').then(r => r.text());
  const ghPathMatch = ghSvg.match(/<path\s+d="([^"]+)"/);
  const ghOctocatPath = ghPathMatch ? ghPathMatch[1] : '';

  fs.writeFileSync(path.join(outDir, 'GitHub-CLI.svg'), template({
    prefix: 'ghc',
    bgColors: ['#121B2A', '#0A101A', '#04070B'],
    glowTop: ['#38BDF8', '#0284C7'],
    glowBottom: ['#818CF8'],
    rim: ['#BAE6FD', '#38BDF8', '#1E293B', '#818CF8'],
    content: `
      <rect x="44" y="56" width="168" height="144" rx="18" fill="#161B22" stroke="#30363D" stroke-width="4" />
      <path d="M44 82H212" stroke="#30363D" stroke-width="2" />
      <circle cx="62" cy="69" r="4.5" fill="#FF5F56" />
      <circle cx="75" cy="69" r="4.5" fill="#FFBD2E" />
      <circle cx="88" cy="69" r="4.5" fill="#27C93F" />
      <path d="M64 116L78 128L64 140" stroke="#38BDF8" stroke-width="6" stroke-linecap="round" stroke-linejoin="round"/>
      <line x1="88" y1="140" x2="108" y2="140" stroke="#34D399" stroke-width="6" stroke-linecap="round"/>
      <svg x="124" y="98" width="76" height="76" viewBox="0 0 24 24" preserveAspectRatio="xMidYMid meet">
        <path fill="#FFFFFF" d="${ghOctocatPath}" />
      </svg>
    `
  }));

  // 10. Driver Booster (Official IObit Driver Booster icon: red gear + curved boost arrow on dark plate)
  const dbPath = "M 113.5 65.0 C 115.8 64.7, 121.8 64.4, 124.0 64.5 C 126.3 64.6, 125.8 64.4, 127.0 65.5 C 128.3 66.6, 130.8 68.1, 131.5 71.0 C 132.3 73.9, 131.3 80.8, 131.5 83.0 C 131.8 85.3, 131.9 84.1, 133.0 84.5 C 134.1 84.9, 136.2 84.8, 138.0 85.5 C 139.8 86.2, 142.5 87.7, 144.0 88.5 C 145.5 89.3, 146.3 89.8, 147.0 90.5 C 147.8 91.3, 150.3 89.8, 148.5 93.0 C 146.7 96.2, 139.8 107.6, 136.0 109.5 C 132.3 111.4, 130.0 105.3, 126.0 104.5 C 122.0 103.7, 115.8 103.8, 112.0 104.5 C 108.2 105.2, 105.9 106.4, 103.0 108.5 C 100.1 110.6, 96.6 114.1, 94.5 117.0 C 92.4 119.9, 91.3 122.8, 90.5 126.0 C 89.7 129.2, 89.5 134.2, 89.5 136.0 C 89.5 137.8, 90.2 135.7, 90.5 137.0 C 90.8 138.3, 90.8 141.8, 91.5 144.0 C 92.2 146.2, 92.6 147.6, 94.5 150.0 C 96.4 152.4, 100.1 156.4, 103.0 158.5 C 105.9 160.6, 108.0 161.8, 112.0 162.5 C 116.0 163.2, 123.0 163.2, 127.0 162.5 C 131.0 161.8, 133.6 160.1, 136.0 158.5 C 138.4 156.9, 140.1 155.1, 141.5 153.0 C 142.9 150.9, 144.0 147.8, 144.5 146.0 C 145.0 144.2, 144.1 143.1, 144.5 142.0 C 144.9 140.9, 146.2 139.7, 147.0 139.5 C 147.8 139.3, 149.1 139.9, 149.5 141.0 C 149.9 142.1, 150.2 142.8, 149.5 146.0 C 148.8 149.2, 147.2 155.7, 145.5 160.0 C 143.8 164.3, 141.3 168.8, 139.5 172.0 C 137.7 175.2, 137.4 175.8, 134.5 179.0 C 131.6 182.3, 125.6 188.4, 122.0 191.5 C 118.4 194.6, 115.2 196.3, 113.0 197.5 C 110.8 198.7, 110.1 198.9, 109.0 198.5 C 107.9 198.1, 106.9 197.4, 106.5 195.0 C 106.1 192.6, 106.8 186.1, 106.5 184.0 C 106.3 181.9, 107.1 183.4, 105.0 182.5 C 102.9 181.6, 96.8 179.8, 94.0 178.5 C 91.2 177.2, 89.8 176.0, 88.0 174.5 C 86.2 173.0, 84.2 170.3, 83.0 169.5 C 81.8 168.7, 82.7 168.7, 81.0 169.5 C 79.3 170.3, 75.3 173.7, 73.0 174.5 C 70.7 175.3, 68.9 175.3, 67.0 174.5 C 65.1 173.8, 63.1 172.1, 61.5 170.0 C 59.9 167.9, 58.2 164.3, 57.5 162.0 C 56.8 159.7, 57.1 157.6, 57.5 156.0 C 57.9 154.4, 58.0 154.0, 60.0 152.5 C 62.0 151.0, 67.9 148.6, 69.5 147.0 C 71.1 145.4, 69.7 143.8, 69.5 143.0 C 69.3 142.2, 68.7 145.0, 68.5 142.0 C 68.3 139.0, 68.2 128.7, 68.5 125.0 C 68.8 121.3, 72.0 122.0, 70.5 120.0 C 69.0 118.0, 61.7 114.7, 59.5 113.0 C 57.3 111.3, 57.8 111.5, 57.5 110.0 C 57.2 108.5, 56.8 106.3, 57.5 104.0 C 58.2 101.7, 59.9 98.1, 61.5 96.0 C 63.1 93.9, 65.1 92.3, 67.0 91.5 C 68.9 90.8, 70.3 90.5, 73.0 91.5 C 75.7 92.5, 80.5 97.3, 83.0 97.5 C 85.5 97.7, 86.2 94.0, 88.0 92.5 C 89.8 91.0, 91.2 89.8, 94.0 88.5 C 96.8 87.2, 102.9 85.4, 105.0 84.5 C 107.1 83.6, 106.3 85.3, 106.5 83.0 C 106.8 80.8, 105.9 73.8, 106.5 71.0 C 107.1 68.3, 108.8 67.5, 110.0 66.5 C 111.2 65.5, 111.2 65.3, 113.5 65.0 Z M 173.5 75.0 C 174.5 74.8, 175.9 74.4, 177.0 74.5 C 178.1 74.6, 178.8 74.8, 180.0 75.5 C 181.3 76.3, 178.9 71.9, 184.5 79.0 C 190.1 86.1, 208.3 110.8, 213.5 118.0 C 218.7 125.2, 215.2 120.7, 215.5 122.0 C 215.8 123.3, 215.8 125.1, 215.5 126.0 C 215.3 126.9, 216.6 127.3, 214.0 127.5 C 211.4 127.8, 203.1 127.1, 200.0 127.5 C 196.9 127.9, 196.8 128.4, 195.5 130.0 C 194.3 131.6, 193.0 135.2, 192.5 137.0 C 192.0 138.8, 193.0 138.7, 192.5 141.0 C 192.0 143.3, 190.8 147.7, 189.5 151.0 C 188.2 154.3, 187.0 157.0, 184.5 161.0 C 182.0 165.0, 178.4 170.4, 174.5 175.0 C 170.6 179.6, 165.8 184.4, 161.0 188.5 C 156.3 192.6, 149.8 197.0, 146.0 199.5 C 142.2 202.0, 140.3 202.5, 138.0 203.5 C 135.7 204.5, 134.2 205.2, 132.0 205.5 C 129.8 205.8, 126.6 206.1, 125.0 205.5 C 123.4 204.9, 122.3 203.2, 122.5 202.0 C 122.7 200.8, 123.4 200.3, 126.0 198.5 C 128.6 196.8, 134.1 194.6, 138.0 191.5 C 141.9 188.4, 146.3 184.3, 149.5 180.0 C 152.8 175.8, 155.5 170.7, 157.5 166.0 C 159.5 161.3, 160.8 155.5, 161.5 152.0 C 162.2 148.5, 161.3 146.3, 161.5 145.0 C 161.7 143.7, 162.3 146.0, 162.5 144.0 C 162.7 142.0, 162.7 135.2, 162.5 133.0 C 162.3 130.8, 162.4 131.9, 161.5 131.0 C 160.6 130.1, 160.6 128.1, 157.0 127.5 C 153.4 126.9, 143.3 128.3, 140.0 127.5 C 136.8 126.8, 137.8 124.6, 137.5 123.0 C 137.3 121.4, 138.0 119.5, 138.5 118.0 C 139.0 116.5, 136.2 120.0, 140.5 114.0 C 144.8 108.0, 159.4 88.4, 164.5 82.0 C 169.6 75.6, 169.5 76.7, 171.0 75.5 C 172.5 74.3, 172.5 75.2, 173.5 75.0 Z M 113.5 116.0 C 116.3 115.3, 121.6 115.3, 124.0 115.5 C 126.4 115.8, 127.1 116.9, 128.0 117.5 C 128.9 118.1, 130.1 117.6, 129.5 119.0 C 128.9 120.4, 125.3 124.0, 124.5 126.0 C 123.7 128.0, 123.8 129.6, 124.5 131.0 C 125.3 132.4, 126.8 133.8, 129.0 134.5 C 131.2 135.2, 136.3 134.1, 137.5 135.0 C 138.8 135.9, 137.2 138.3, 136.5 140.0 C 135.8 141.7, 134.6 143.6, 133.5 145.0 C 132.4 146.4, 131.6 147.4, 130.0 148.5 C 128.4 149.6, 126.5 151.0, 124.0 151.5 C 121.5 152.0, 117.3 151.8, 115.0 151.5 C 112.7 151.2, 111.6 150.4, 110.0 149.5 C 108.4 148.6, 106.9 147.8, 105.5 146.0 C 104.1 144.3, 102.3 141.2, 101.5 139.0 C 100.7 136.8, 100.2 135.5, 100.5 133.0 C 100.8 130.5, 102.4 126.3, 103.5 124.0 C 104.6 121.8, 105.3 120.8, 107.0 119.5 C 108.7 118.2, 110.7 116.7, 113.5 116.0 Z";

  fs.writeFileSync(path.join(outDir, 'Driver-Booster.svg'), template({
    prefix: 'db',
    bgColors: ['#2A0E12', '#160608', '#0A0203'],
    glowTop: ['#EF4444', '#DC2626'],
    glowBottom: ['#B91C1C'],
    rim: ['#FECACA', '#EF4444', '#450A0A', '#B91C1C'],
    content: `
      <defs>
        <linearGradient id="db-disc" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stop-color="#242426" />
          <stop offset="100%" stop-color="#141416" />
        </linearGradient>
        <linearGradient id="db-red" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stop-color="#FF383F" />
          <stop offset="100%" stop-color="#DC2626" />
        </linearGradient>
      </defs>
      <!-- Outer red circular ring and dark inner plate matching official IObit icon -->
      <circle cx="128" cy="128" r="82" fill="url(#db-disc)" stroke="url(#db-red)" stroke-width="11" />
      <!-- Gear and Boost Arrow -->
      <path fill="url(#db-red)" fill-rule="evenodd" d="${dbPath}" />
    `
  }));

  // 11. Microsoft Edge
  const edgeSvg = await fetch('https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/microsoftedge.svg').then(r => r.text());
  const edgePathMatch = edgeSvg.match(/<path\s+d="([^"]+)"/);
  const edgePath = edgePathMatch ? edgePathMatch[1] : '';

  fs.writeFileSync(path.join(outDir, 'Microsoft-Edge.svg'), template({
    prefix: 'edg',
    bgColors: ['#072138', '#041321', '#01080E'],
    glowTop: ['#0078D4', '#0284C7'],
    glowBottom: ['#107C41'],
    rim: ['#BAE6FD', '#0078D4', '#064E3B', '#34D399'],
    content: `<svg x="50" y="50" width="156" height="156" viewBox="0 0 24 24" preserveAspectRatio="xMidYMid meet">
      <defs>
        <linearGradient id="edg-mark" x1="0" y1="0" x2="24" y2="24" gradientUnits="userSpaceOnUse">
          <stop offset="0%" stop-color="#00C7FF" />
          <stop offset="50%" stop-color="#0078D4" />
          <stop offset="100%" stop-color="#107C41" />
        </linearGradient>
      </defs>
      <path fill="url(#edg-mark)" d="${edgePath}" />
    </svg>`
  }));

  // 12. Blender
  const bldSvg = await fetch('https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/blender.svg').then(r => r.text());
  const bldPathMatch = bldSvg.match(/<path\s+d="([^"]+)"/);
  const bldPath = bldPathMatch ? bldPathMatch[1] : '';

  fs.writeFileSync(path.join(outDir, 'Blender.svg'), template({
    prefix: 'bld',
    bgColors: ['#2B180D', '#170D06', '#0A0503'],
    glowTop: ['#EA7600', '#C2410C'],
    glowBottom: ['#2274A5'],
    rim: ['#FED7AA', '#EA7600', '#1E3A8A', '#60A5FA'],
    content: `<svg x="50" y="50" width="156" height="156" viewBox="0 0 24 24" preserveAspectRatio="xMidYMid meet">
      <defs>
        <linearGradient id="bld-mark" x1="0" y1="0" x2="24" y2="24" gradientUnits="userSpaceOnUse">
          <stop offset="0%" stop-color="#FF9800" />
          <stop offset="100%" stop-color="#EA7600" />
        </linearGradient>
      </defs>
      <path fill="url(#bld-mark)" d="${bldPath}" />
    </svg>`
  }));

  // 13. NanaZip
  fs.writeFileSync(path.join(outDir, 'NanaZip.svg'), template({
    prefix: 'nnz',
    bgColors: ['#0F203D', '#081224', '#030811'],
    glowTop: ['#38BDF8', '#0284C7'],
    glowBottom: ['#818CF8'],
    rim: ['#BAE6FD', '#38BDF8', '#1E1B4B', '#818CF8'],
    content: `
      <defs>
        <linearGradient id="nnz-shield" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stop-color="#38BDF8" />
          <stop offset="100%" stop-color="#0284C7" />
        </linearGradient>
        <linearGradient id="nnz-inner" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stop-color="#0F172A" />
          <stop offset="100%" stop-color="#1E293B" />
        </linearGradient>
      </defs>
      <path d="M128 52L192 76V132C192 170 164 198 128 208C92 198 64 170 64 132V76L128 52Z" fill="url(#nnz-shield)" stroke="#BAE6FD" stroke-width="4"/>
      <path d="M128 66L180 86V130C180 160 158 184 128 194C98 184 76 160 76 130V86L128 66Z" fill="url(#nnz-inner)"/>
      <!-- Zipper teeth -->
      <rect x="122" y="80" width="12" height="6" rx="2" fill="#BAE6FD"/>
      <rect x="122" y="92" width="12" height="6" rx="2" fill="#BAE6FD"/>
      <rect x="122" y="104" width="12" height="6" rx="2" fill="#BAE6FD"/>
      <rect x="122" y="116" width="12" height="6" rx="2" fill="#BAE6FD"/>
      <!-- 7Z Typography -->
      <text x="128" y="162" font-family="system-ui, -apple-system, sans-serif" font-size="28" font-weight="900" fill="#FFFFFF" text-anchor="middle" letter-spacing="1">7Z</text>
    `
  }));

  // 14. Wireshark
  const wskSvg = await fetch('https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/wireshark.svg').then(r => r.text());
  const wskPathMatch = wskSvg.match(/<path\s+d="([^"]+)"/);
  const wskPath = wskPathMatch ? wskPathMatch[1] : '';

  fs.writeFileSync(path.join(outDir, 'Wireshark.svg'), template({
    prefix: 'wsk',
    bgColors: ['#0A1D38', '#051020', '#02070E'],
    glowTop: ['#0084C9', '#0284C7'],
    glowBottom: ['#38BDF8'],
    rim: ['#BAE6FD', '#0084C9', '#082F49', '#38BDF8'],
    content: `<svg x="50" y="50" width="156" height="156" viewBox="0 0 24 24" preserveAspectRatio="xMidYMid meet">
      <defs>
        <linearGradient id="wsk-mark" x1="0" y1="0" x2="24" y2="24" gradientUnits="userSpaceOnUse">
          <stop offset="0%" stop-color="#38BDF8" />
          <stop offset="100%" stop-color="#0084C9" />
        </linearGradient>
      </defs>
      <path fill="url(#wsk-mark)" d="${wskPath}" />
    </svg>`
  }));

  // 15. Rust
  const rustSvg = await fetch('https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/rust.svg').then(r => r.text());
  const rustPathMatch = rustSvg.match(/<path\s+d="([^"]+)"/);
  const rustPath = rustPathMatch ? rustPathMatch[1] : '';

  fs.writeFileSync(path.join(outDir, 'Rust.svg'), template({
    prefix: 'rust',
    bgColors: ['#28160E', '#170C07', '#0A0503'],
    glowTop: ['#F97316', '#C2410C'],
    glowBottom: ['#EF4444'],
    rim: ['#FFEDD5', '#F97316', '#450A0A', '#EF4444'],
    content: `<svg x="50" y="50" width="156" height="156" viewBox="0 0 24 24" preserveAspectRatio="xMidYMid meet">
      <defs>
        <linearGradient id="rust-mark" x1="0" y1="0" x2="24" y2="24" gradientUnits="userSpaceOnUse">
          <stop offset="0%" stop-color="#FED7AA" />
          <stop offset="50%" stop-color="#F97316" />
          <stop offset="100%" stop-color="#EA580C" />
        </linearGradient>
      </defs>
      <path fill="url(#rust-mark)" d="${rustPath}" />
    </svg>`
  }));

  // 16. Go
  const goSvg = await fetch('https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/go.svg').then(r => r.text());
  const goPathMatch = goSvg.match(/<path\s+d="([^"]+)"/);
  const goPath = goPathMatch ? goPathMatch[1] : '';

  fs.writeFileSync(path.join(outDir, 'Go.svg'), template({
    prefix: 'golang',
    bgColors: ['#0A2233', '#05131D', '#02070B'],
    glowTop: ['#00ADD8', '#0284C7'],
    glowBottom: ['#38BDF8'],
    rim: ['#BAE6FD', '#00ADD8', '#082F49', '#38BDF8'],
    content: `<svg x="50" y="50" width="156" height="156" viewBox="0 0 24 24" preserveAspectRatio="xMidYMid meet">
      <defs>
        <linearGradient id="go-mark" x1="0" y1="0" x2="24" y2="24" gradientUnits="userSpaceOnUse">
          <stop offset="0%" stop-color="#38BDF8" />
          <stop offset="100%" stop-color="#00ADD8" />
        </linearGradient>
      </defs>
      <path fill="url(#go-mark)" d="${goPath}" />
    </svg>`
  }));

  // 17. Discord Canary
  const dscClyde = "M216.856 16.597A208.5 208.5 0 0 0 164.042 0c-2.275 4.113-4.933 9.645-6.766 14.046q-29.538-4.442-58.533 0c-1.832-4.4-4.55-9.933-6.846-14.046a207.8 207.8 0 0 0-52.855 16.638C5.618 67.147-3.443 116.4 1.087 164.956c22.169 16.555 43.653 26.612 64.775 33.193A161 161 0 0 0 79.735 175.3a136.4 136.4 0 0 1-21.846-10.632a109 109 0 0 0 5.356-4.237c42.122 19.702 87.89 19.702 129.51 0a132 132 0 0 0 5.355 4.237a136 136 0 0 1-21.886 10.653c4.006 8.02 8.638 15.67 13.873 22.848c21.142-6.58 42.646-16.637 64.815-33.213c5.316-56.288-9.08-105.09-38.056-148.36M85.474 135.095c-12.645 0-23.015-11.805-23.015-26.18s10.149-26.2 23.015-26.2s23.236 11.804 23.015 26.2c.02 14.375-10.148 26.18-23.015 26.18m85.051 0c-12.645 0-23.014-11.805-23.014-26.18s10.148-26.2 23.014-26.2c12.867 0 23.236 11.804 23.015 26.2c0 14.375-10.148 26.18-23.015 26.18";
  fs.writeFileSync(path.join(outDir, 'Discord-Canary.svg'), template({
    prefix: 'canary',
    bgColors: ['#302507', '#1B1403', '#0B0801'],
    glowTop: ['#FACC15', '#CA8A04'],
    glowBottom: ['#F59E0B'],
    rim: ['#FEF08A', '#FACC15', '#451A03', '#F59E0B'],
    content: `<svg x="50" y="50" width="156" height="156" viewBox="0 0 256 199" preserveAspectRatio="xMidYMid meet">
      <defs>
        <linearGradient id="canary-mark" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stop-color="#FEF08A" />
          <stop offset="100%" stop-color="#EAB308" />
        </linearGradient>
      </defs>
      <path fill="url(#canary-mark)" d="${dscClyde}" />
    </svg>`
  }));

  // 18. ChatGPT
  const gptSvg = await fetch('https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/openai.svg').then(r => r.text());
  const gptPathMatch = gptSvg.match(/<path\s+d="([^"]+)"/);
  const gptPath = gptPathMatch ? gptPathMatch[1] : '';

  fs.writeFileSync(path.join(outDir, 'ChatGPT.svg'), template({
    prefix: 'gpt',
    bgColors: ['#0C2720', '#061612', '#020A08'],
    glowTop: ['#10A37F', '#059669'],
    glowBottom: ['#34D399'],
    rim: ['#A7F3D0', '#10A37F', '#064E3B', '#34D399'],
    content: `<svg x="50" y="50" width="156" height="156" viewBox="0 0 24 24" preserveAspectRatio="xMidYMid meet">
      <path fill="#FFFFFF" d="${gptPath}" />
    </svg>`
  }));

  // 19. Claude
  const cldSvg = await fetch('https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/anthropic.svg').then(r => r.text());
  const cldPathMatch = cldSvg.match(/<path\s+d="([^"]+)"/);
  const cldPath = cldPathMatch ? cldPathMatch[1] : '';

  fs.writeFileSync(path.join(outDir, 'Claude.svg'), template({
    prefix: 'cld',
    bgColors: ['#2B170E', '#180B06', '#0B0402'],
    glowTop: ['#D97706', '#B45309'],
    glowBottom: ['#EA580C'],
    rim: ['#FED7AA', '#D97706', '#431407', '#EA580C'],
    content: `<svg x="52" y="52" width="152" height="152" viewBox="0 0 24 24" preserveAspectRatio="xMidYMid meet">
      <defs>
        <linearGradient id="cld-mark" x1="0" y1="0" x2="24" y2="24" gradientUnits="userSpaceOnUse">
          <stop offset="0%" stop-color="#FED7AA" />
          <stop offset="100%" stop-color="#D97706" />
        </linearGradient>
      </defs>
      <path fill="url(#cld-mark)" d="${cldPath}" />
    </svg>`
  }));

  console.log('Successfully generated all 19 custom icons in ' + outDir);
}

main().catch(err => {
  console.error(err);
  process.exit(1);
});
