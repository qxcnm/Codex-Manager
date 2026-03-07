export function activateAccountsPage({
  accountPageLoaded,
  renderCurrentPageView,
  reloadAccountsPage,
  reloadOptions = {},
}) {
  if (accountPageLoaded === true) {
    renderCurrentPageView("accounts");
  }

  return reloadAccountsPage({
    silent: true,
    latestOnly: true,
    ...reloadOptions,
  });
}
