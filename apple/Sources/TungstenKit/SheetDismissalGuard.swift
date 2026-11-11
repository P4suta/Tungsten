#if os(iOS)
  import SwiftUI
  import UIKit

  struct SheetDismissalGuard: UIViewControllerRepresentable {
    let isModified: Bool
    let onAttempt: () -> Void

    func makeUIViewController(context: Context) -> Controller { Controller() }

    func updateUIViewController(_ controller: Controller, context: Context) {
      controller.isModified = isModified
      controller.onAttempt = onAttempt
      controller.install()
    }

    final class Controller: UIViewController, UIAdaptivePresentationControllerDelegate {
      var isModified = false
      var onAttempt: () -> Void = {}
      private weak var originalDelegate: (any UIAdaptivePresentationControllerDelegate)?

      override func didMove(toParent parent: UIViewController?) {
        super.didMove(toParent: parent)
        install()
      }

      override func viewDidAppear(_ animated: Bool) {
        super.viewDidAppear(animated)
        install()
      }

      func install() {
        guard var host = parent else { return }
        while let ancestor = host.parent { host = ancestor }
        guard let presentation = host.presentationController else { return }
        if presentation.delegate !== self {
          originalDelegate = presentation.delegate
          presentation.delegate = self
        }
        host.isModalInPresentation = isModified
      }

      func presentationControllerShouldDismiss(_ presentationController: UIPresentationController)
        -> Bool
      {
        !isModified
          && (originalDelegate?.presentationControllerShouldDismiss?(presentationController) ?? true)
      }

      func presentationControllerDidAttemptToDismiss(
        _ presentationController: UIPresentationController
      ) {
        if isModified {
          onAttempt()
        } else {
          originalDelegate?.presentationControllerDidAttemptToDismiss?(presentationController)
        }
      }

      func presentationControllerWillDismiss(_ presentationController: UIPresentationController) {
        originalDelegate?.presentationControllerWillDismiss?(presentationController)
      }

      func presentationControllerDidDismiss(_ presentationController: UIPresentationController) {
        originalDelegate?.presentationControllerDidDismiss?(presentationController)
      }
    }
  }
#endif
