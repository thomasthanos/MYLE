// MYLE sign-in start page: a Cloudflare Worker on your own domain (e.g.
// login.thomast.uk) that sends the sign-in on to Supabase with the same
// query. iOS's "MYLE Passwords Wants to Use … to Sign In" prompt names the
// host the sign-in starts on, so it shows "thomast.uk" instead of the
// Supabase project's address.
//
// Only /authorize is answered, and only ever to this Supabase project, so it
// cannot be used to send people anywhere else.
//
// Deploy: Cloudflare dashboard › Workers › Create › paste this › add the
// custom domain login.thomast.uk. Then set the GitHub repository variable
// MYLE_AUTH_URL = https://login.thomast.uk/authorize and release again.

const SUPABASE = "https://oofcywdbmhmqpowmwykz.supabase.co";
const ALLOWED = new Set(["provider", "redirect_to", "code_challenge", "code_challenge_method", "scopes"]);

export default {
  async fetch(request) {
    const url = new URL(request.url);
    if (url.pathname !== "/authorize") return new Response("Not found", { status: 404 });
    const target = new URL(`${SUPABASE}/auth/v1/authorize`);
    for (const [key, value] of url.searchParams) {
      if (ALLOWED.has(key)) target.searchParams.append(key, value);
    }
    if (!target.searchParams.get("provider")) return new Response("Missing provider", { status: 400 });
    return Response.redirect(target.toString(), 302);
  },
};
