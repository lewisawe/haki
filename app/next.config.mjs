/** @type {import('next').NextConfig} */
const nextConfig = {
  reactStrictMode: true,
  // @solana/wallet-adapter and web3.js pull a couple of node polyfill shaped
  // deps; keep the build permissive so a flaky install doesn't block dev.
  webpack: (config) => {
    config.resolve.fallback = { ...config.resolve.fallback, fs: false };
    return config;
  },
};

export default nextConfig;
