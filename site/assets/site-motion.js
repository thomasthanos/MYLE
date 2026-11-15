const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)");

function activatePreview() {
  const tablist = document.querySelector(".preview-tabs");
  if (!tablist) return;

  const tabs = [...tablist.querySelectorAll('[role="tab"]')];
  const panels = tabs.map((tab) => document.getElementById(tab.getAttribute("aria-controls")));
  if (!tabs.length || panels.some((panel) => !panel)) return;

  function select(index, moveFocus) {
    tabs.forEach((tab, tabIndex) => {
      const selected = index === tabIndex;
      tab.setAttribute("aria-selected", String(selected));
      tab.tabIndex = selected ? 0 : -1;
      panels[tabIndex].hidden = !selected;
    });
    if (moveFocus) tabs[index].focus({ preventScroll: true });
  }

  tablist.addEventListener("click", (event) => {
    const index = tabs.indexOf(event.target.closest('[role="tab"]'));
    if (index >= 0) select(index, false);
  });

  tablist.addEventListener("keydown", (event) => {
    const index = tabs.indexOf(event.target);
    if (index < 0) return;

    let next;
    switch (event.key) {
      case "ArrowLeft": next = (index + tabs.length - 1) % tabs.length; break;
      case "ArrowRight": next = (index + 1) % tabs.length; break;
      case "Home": next = 0; break;
      case "End": next = tabs.length - 1; break;
      default: return;
    }
    event.preventDefault();
    select(next, true);
  });
  tablist.hidden = false;
}

function revealSections() {
  if (reducedMotion.matches || !("IntersectionObserver" in window)) return;

  const reveal = new IntersectionObserver((entries) => {
    for (const { target, isIntersecting } of entries) {
      if (!isIntersecting) continue;
      target.classList.add("motion-reveal");
      reveal.unobserve(target);
    }
  }, { rootMargin: "0px 0px -5% 0px", threshold: 0.08 });

  const sections = ".section-heading, .feature-card, .download-panel, .info-card, .release-card, .notice";
  document.querySelectorAll(sections).forEach((section) => reveal.observe(section));

  const releases = document.getElementById("releases");
  if (!releases) return;
  // The changelog cards arrive after the GitHub request finishes.
  new MutationObserver((records) => {
    for (const record of records) {
      for (const card of record.addedNodes) {
        if (card instanceof HTMLElement && card.matches(".release-card")) reveal.observe(card);
      }
    }
  }).observe(releases, { childList: true });
}

activatePreview();
revealSections();
