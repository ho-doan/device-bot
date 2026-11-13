'use strict';
// Shared by the report UI and its tests. Aliases are one run, not extra results.
const ReportHistory = {
  merge(groups, selected, mode = 'any') {
    const runs = new Map();
    for (const group of groups) {
      for (const run of group.runs) {
        let entry = runs.get(run.id);
        if (!entry) {
          entry = { ...run, projectId: group.projectId, projectIds: [] };
          runs.set(run.id, entry);
        }
        if (!entry.projectIds.includes(group.projectId)) entry.projectIds.push(group.projectId);
      }
    }
    return [...runs.values()]
      .filter(run => mode !== 'all' || selected.every(id => run.projectIds.includes(id)))
      .sort((a, b) => b.lastReportAt - a.lastReportAt || a.id.localeCompare(b.id));
  },
};
if (typeof module !== 'undefined') module.exports = ReportHistory;
