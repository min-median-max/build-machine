// Resolve only the running Build Machine window for native screen capture.
ObjC.import('AppKit');
ObjC.import('CoreGraphics');
const applications = $.NSWorkspace.sharedWorkspace.runningApplications;
let processId = null;
for (let index = 0; index < applications.count; index++) {
  const application = applications.objectAtIndex(index);
  if (ObjC.unwrap(application.bundleIdentifier) === 'local.buildmachine.desktop') {
    processId = application.processIdentifier;
  }
}
const windows = ObjC.deepUnwrap(ObjC.castRefToObject($.CGWindowListCopyWindowInfo(17, 0)));
const window = windows.find(item => item.kCGWindowOwnerPID === processId && item.kCGWindowLayer === 0);
if (!window) throw new Error('Open the Build Machine window before capturing it.');
String(window.kCGWindowNumber);
